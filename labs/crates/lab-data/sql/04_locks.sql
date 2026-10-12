-- Lab P12a, PostgreSQL part 4: who blocks whom (pg_locks, pg_blocking_pids), the DDL lock queue, deadlock.
-- Theory: docs/12-he-thong-du-lieu/15-postgresql-ben-trong.md (section 7), 07-transaction.md (section 7).
--
-- STATUS: NOT RUN (no Docker / psql was used by the author). "Expect" lines are predictions from the
-- PostgreSQL 16 documentation, not recorded output.
--
-- Running this file with -f only creates the table. The scenarios are commented; paste them into up to four psql
-- sessions A, B, C, D (C and D only observe), removing the leading "-- [A] " etc. Order matters.
--   docker exec -it lab-pg psql -U postgres        (open four of these)
-- Setup:
--   docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/04_locks.sql

DROP TABLE IF EXISTS lockdemo;
CREATE TABLE lockdemo (id int PRIMARY KEY, v int NOT NULL);
INSERT INTO lockdemo VALUES (1, 0), (2, 0);

-- ===== Part 1: a row lock, seen from outside ===================================================
-- [A] BEGIN;
-- [A] UPDATE lockdemo SET v = v + 1 WHERE id = 1;
-- [A] SELECT pg_backend_pid() AS pid_a, pg_current_xact_id() AS xid_a;     -- write both down
-- [B] BEGIN;
-- [B] SELECT pg_backend_pid() AS pid_b;                                    -- write it down
-- [B] UPDATE lockdemo SET v = v + 10 WHERE id = 1;                         -- does not return: waits for A
-- [C] SELECT pid, locktype, mode, granted, relation::regclass AS rel, transactionid
--       FROM pg_locks WHERE pid IN (<pid_a>, <pid_b>) AND locktype IN ('relation', 'transactionid', 'tuple')
--      ORDER BY pid, locktype;
--     Expect: A holds RowExclusiveLock on lockdemo and on lockdemo_pkey, and an ExclusiveLock on its own
--     transactionid (granted = t). B also holds RowExclusiveLock on the relations, and has a row with
--     locktype = transactionid, transactionid = xid_a, mode = ShareLock, granted = f: B waits for A's
--     transaction to end. (The row lock itself is not in this view: it is stored in the tuple on disk.)
-- [C] SELECT pid, wait_event_type, wait_event, pg_blocking_pids(pid) AS blocked_by, state, left(query, 45) AS query
--       FROM pg_stat_activity WHERE datname = current_database() AND pid <> pg_backend_pid();
--     Expect: B has wait_event_type = Lock, wait_event = transactionid, blocked_by = {pid_a}; A is
--     'idle in transaction' and not waiting for any lock.
-- [A] COMMIT;
--     Expect: B's UPDATE returns at once (UPDATE 1). [B] COMMIT; then v = 11.

-- ===== Part 2: a DDL statement waiting in the lock queue blocks everyone behind it ==============
-- [A] BEGIN;
-- [A] SELECT count(*) FROM lockdemo;                  -- holds ACCESS SHARE until the transaction ends
--                                                     -- (A stays 'idle in transaction': the classic forgotten transaction)
-- [B] ALTER TABLE lockdemo ADD COLUMN note text;      -- needs ACCESS EXCLUSIVE: waits for A
-- [C] SELECT * FROM lockdemo;                         -- a plain read; does not conflict with A, yet it hangs
-- [D] SELECT pid, wait_event_type, wait_event, pg_blocking_pids(pid), left(query, 40) AS query
--       FROM pg_stat_activity WHERE wait_event_type = 'Lock';
--     Expect: two waiting processes: the ALTER TABLE (blocked by A) and the plain SELECT, whose
--     pg_blocking_pids contains the ALTER TABLE's pid because that request is ahead of it in the queue
--     and conflicts with it (a "soft block").
-- [A] ROLLBACK;                                       -- ALTER TABLE gets the lock first, then the SELECT runs
--
-- The mitigation: give DDL a lock_timeout so it fails fast instead of queueing.
-- [A] BEGIN;  SELECT count(*) FROM lockdemo;
-- [B] SET lock_timeout = '2s';
-- [B] ALTER TABLE lockdemo ADD COLUMN note2 text;
--     Expect: after about 2 seconds, ERROR: canceling statement due to lock timeout. Nobody else was
--     blocked for longer than that. Retry later, when A is gone.
-- [A] ROLLBACK;

-- ===== Part 3: a deadlock =====================================================================
-- [A] BEGIN;  UPDATE lockdemo SET v = v + 1 WHERE id = 1;
-- [B] BEGIN;  UPDATE lockdemo SET v = v + 1 WHERE id = 2;
-- [A] UPDATE lockdemo SET v = v + 1 WHERE id = 2;     -- waits for B
-- [B] UPDATE lockdemo SET v = v + 1 WHERE id = 1;     -- closes the cycle
--     Expect: nothing happens for about deadlock_timeout (default 1 s), then one of the two sessions gets
--     ERROR: deadlock detected (SQLSTATE 40P01) and its transaction is aborted; the other one proceeds.
--     The aborted application must ROLLBACK and retry the whole transaction.
-- Fix: make both sessions update id = 1 first, then id = 2 (a consistent lock order); the second session
-- then simply waits and no cycle exists.
-- Roll back whatever is still open in A and B.
