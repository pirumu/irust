-- Lab P12a, PostgreSQL part 2: lost update and write skew (two sessions).
-- Theory: docs/12-he-thong-du-lieu/07-transaction.md (sections 3, 5 and 7), 15-postgresql-ben-trong.md (section 5).
-- Model counterpart: `cargo run -q --release -p lab-data --bin mvcc` (the same scenarios on the toy engine).
--
-- STATUS: NOT RUN (no Docker / psql was used by the author). "Expect" lines are predictions from the
-- PostgreSQL 16 documentation, not recorded output.
--
-- Running this file with -f only creates the tables. The scenarios are commented; paste them step by
-- step into two psql sessions A and B (order matters), removing the leading "-- [A] " / "-- [B] ":
--   docker exec -it lab-pg psql -U postgres        (session A)
--   docker exec -it lab-pg psql -U postgres        (session B)
-- Setup:
--   docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/02_write_skew.sql

DROP TABLE IF EXISTS doctors;
CREATE TABLE doctors (name text PRIMARY KEY, on_call boolean NOT NULL);
INSERT INTO doctors VALUES ('alice', true), ('bob', true);

DROP TABLE IF EXISTS counter;
CREATE TABLE counter (id int PRIMARY KEY, n int NOT NULL);
INSERT INTO counter VALUES (1, 0);

-- ===== Round 1: lost update, application-level read-modify-write, READ COMMITTED ==============
-- [A] BEGIN;
-- [A] SELECT n FROM counter WHERE id = 1;            -- 0
-- [B] BEGIN;
-- [B] SELECT n FROM counter WHERE id = 1;            -- 0
-- [A] UPDATE counter SET n = 1 WHERE id = 1;         -- the application computed 0 + 1
-- [A] COMMIT;
-- [B] UPDATE counter SET n = 1 WHERE id = 1;         -- B also computed 0 + 1 from its stale read
-- [B] COMMIT;
-- [A] SELECT n FROM counter WHERE id = 1;
--     Expect: n = 1 although two increments were committed: one was lost.
--
-- Same steps with BEGIN ISOLATION LEVEL REPEATABLE READ in both sessions (reset first:
--   UPDATE counter SET n = 0;):
--     Expect: B's UPDATE fails with ERROR: could not serialize access due to concurrent update (40001),
--     B must ROLLBACK and retry the whole transaction; the final n is 1 after A and 2 after B's retry.
--
-- Atomic form: UPDATE counter SET n = n + 1 WHERE id = 1;  in both sessions at READ COMMITTED.
--     Expect: the second UPDATE waits for the first to commit, then re-reads the new row version
--     and adds 1 to it; final n = 2 (reset first).

-- ===== Round 2: write skew under REPEATABLE READ (snapshot isolation) =========================
-- Invariant the application wants: at least one doctor stays on call.
-- [A] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [A] SELECT count(*) FROM doctors WHERE on_call;     -- 2
-- [B] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [B] SELECT count(*) FROM doctors WHERE on_call;     -- 2
-- [A] UPDATE doctors SET on_call = false WHERE name = 'alice';
-- [B] UPDATE doctors SET on_call = false WHERE name = 'bob';   -- different row: no conflict, no wait
-- [A] COMMIT;
-- [B] COMMIT;
-- [A] SELECT count(*) FROM doctors WHERE on_call;
--     Expect: both COMMITs succeed and the count is 0: the invariant is broken. This is write skew.
--     (reset: UPDATE doctors SET on_call = true;)

-- ===== Round 3: the same steps under SERIALIZABLE ==============================================
-- Use BEGIN ISOLATION LEVEL SERIALIZABLE in both sessions, same statements.
--     Expect: one of the two transactions fails with
--       ERROR: could not serialize access due to read/write dependencies among transactions (40001)
--     (at its UPDATE or at its COMMIT; which one depends on timing). Roll it back and retry it from BEGIN:
--     the retried transaction now sees count = 1 and must not take the last doctor off call (that check is
--     your application's job). The final count is 1, never 0.
--     (reset: UPDATE doctors SET on_call = true;)

-- ===== Round 4: fix at REPEATABLE READ with explicit row locks =================================
-- [A] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [A] SELECT * FROM doctors WHERE on_call FOR UPDATE;      -- locks alice and bob
-- [B] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [B] SELECT * FROM doctors WHERE on_call FOR UPDATE;      -- blocks: B waits for A's row locks
-- [A] UPDATE doctors SET on_call = false WHERE name = 'alice';
-- [A] COMMIT;
--     Expect: B's SELECT ... FOR UPDATE now fails with ERROR: could not serialize access due to
--     concurrent update (40001): a REPEATABLE READ transaction cannot lock a row that changed after
--     its snapshot. B rolls back, retries, sees one doctor on call, and must stop.
--     (The same code at READ COMMITTED does not fail: B re-checks the row and sees on_call = false.)
