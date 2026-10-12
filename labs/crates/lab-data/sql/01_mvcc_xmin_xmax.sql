-- Lab P12a, PostgreSQL part 1: row versions, xmin / xmax / ctid, snapshots.
-- Theory: docs/12-he-thong-du-lieu/07-transaction.md (section 4) and 15-postgresql-ben-trong.md (sections 4-5).
--
-- STATUS: NOT RUN. The author ran only `cargo` on macOS for this lab (no Docker, no psql). Every
-- "Expect" below is a prediction derived from the PostgreSQL 16 documentation and source, not a
-- recorded output. Compare it with what your own server prints.
--
-- Start a server (once):
--   docker run --name lab-pg -e POSTGRES_PASSWORD=lab -d postgres:16
-- Run the single-session part of this file:
--   docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/01_mvcc_xmin_xmax.sql
-- The two-session part at the end is commented out; paste it into two terminals:
--   docker exec -it lab-pg psql -U postgres        (session A)
--   docker exec -it lab-pg psql -U postgres        (session B)

\echo '== 0. a table with one row'
DROP TABLE IF EXISTS acct;
CREATE TABLE acct (id int PRIMARY KEY, balance int NOT NULL);
INSERT INTO acct VALUES (1, 100);

\echo '== 1. the hidden system columns of that row version'
SELECT ctid, xmin, xmax, * FROM acct;
-- Expect: one row. ctid = (0,1): page 0, line pointer 1. xmax = 0: nobody has deleted or replaced it.
-- xmin = the transaction id of the INSERT.

\echo '== 2. an UPDATE writes a NEW row version'
BEGIN;
SELECT pg_current_xact_id() AS my_xid;        -- assigns this transaction an id
UPDATE acct SET balance = balance - 20 WHERE id = 1;
SELECT ctid, xmin, xmax, * FROM acct;
COMMIT;
-- Expect inside the transaction: a different ctid (the docs say "a row's ctid will change if it is updated"),
-- xmin = my_xid, xmax = 0. The old version is not visible any more.

\echo '== 3. the old version is still in the page (pageinspect shows every tuple, visible or not)'
CREATE EXTENSION IF NOT EXISTS pageinspect;
SELECT lp, t_xmin, t_xmax, t_ctid FROM heap_page_items(get_raw_page('acct', 0));
-- Expect two line pointers (lp 1 and 2), one tuple each:
--   lp 1, the old version: t_xmin = the INSERT's xid, t_xmax = my_xid, t_ctid -> the new version
--   lp 2, the new version: t_xmin = my_xid,          t_xmax = 0
-- (the updater's xid is stored in the old tuple's xmax, and the new tuple's address in the old tuple's t_ctid)

\echo '== 4. a snapshot is text: xmin:xmax:xip_list'
BEGIN ISOLATION LEVEL REPEATABLE READ;
SELECT pg_current_snapshot();                  -- the first statement of the transaction takes the snapshot
SELECT balance FROM acct WHERE id = 1;
COMMIT;
-- Expect a text of the form xmin:xmax:xip_list. xmin = lowest transaction id still active, xmax = one past the
-- highest completed id, xip_list = the active ids in between (probably empty, and xmin = xmax, on an idle server).

-- ---------------------------------------------------------------------------------------------
-- Two sessions (remove the comment marker when you paste into psql). Order matters.
-- ---------------------------------------------------------------------------------------------
-- [A] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [A] SELECT ctid, xmin, xmax, * FROM acct;          -- snapshot taken now; balance 80
-- [B] UPDATE acct SET balance = 50 WHERE id = 1;     -- autocommit
-- [A] SELECT ctid, xmin, xmax, * FROM acct;
--     Expect: still balance 80 (the transaction only sees data committed before its snapshot), and the very same
--     row version A already saw, now with xmax = B's transaction id (B committed after A's snapshot).
-- [A] UPDATE acct SET balance = balance + 1 WHERE id = 1;
--     Expect: ERROR: could not serialize access due to concurrent update    (SQLSTATE 40001)
-- [A] ROLLBACK;
--
-- The same interleaving at READ COMMITTED:
-- [A] BEGIN;                                          -- default isolation level
-- [A] SELECT balance FROM acct WHERE id = 1;          -- 50
-- [B] UPDATE acct SET balance = 10 WHERE id = 1;
-- [A] UPDATE acct SET balance = balance + 1 WHERE id = 1;
--     Expect: no error; the statement takes a fresh snapshot, sees B's version, and the row becomes 11.
-- [A] COMMIT;
