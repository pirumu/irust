-- Lab P12a, PostgreSQL part 3: dead row versions, VACUUM, and what a long transaction does to it.
-- Theory: docs/12-he-thong-du-lieu/07-transaction.md (section 4), 15-postgresql-ben-trong.md (sections 4 and 6).
-- Model counterpart: `cargo run -q --release -p lab-data --bin mvcc` (the "vacuum" section).
--
-- STATUS: NOT RUN (no Docker / psql was used by the author). "Expect" lines are predictions from the
-- PostgreSQL 16 documentation and source, not recorded output. Sizes and counts depend on page
-- pruning, on statistics reporting delay and on your machine; read them as orders of magnitude.
--
-- Run the single-session part:
--   docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/03_vacuum_dead_tuples.sql
-- The two-session part at the end is commented out (sessions A and B, as in 02_write_skew.sql).

\echo '== 0. a table autovacuum will not touch, so that we decide when VACUUM runs'
DROP TABLE IF EXISTS churn;
CREATE TABLE churn (id int PRIMARY KEY, v int NOT NULL) WITH (autovacuum_enabled = false);
INSERT INTO churn SELECT g, 0 FROM generate_series(1, 100000) AS g;
SELECT pg_size_pretty(pg_relation_size('churn'))      AS heap,
       pg_size_pretty(pg_relation_size('churn_pkey')) AS pkey;

\echo '== 1. three full-table updates: every UPDATE writes a new version of every row'
UPDATE churn SET v = v + 1;
UPDATE churn SET v = v + 1;
UPDATE churn SET v = v + 1;
SELECT pg_size_pretty(pg_relation_size('churn'))      AS heap_after_updates,
       pg_size_pretty(pg_relation_size('churn_pkey')) AS pkey_after_updates;
-- Expect: the heap is clearly bigger than in step 0. The upper bound is about 4x (four versions of each row);
-- page pruning can reclaim some space inside a page, so the real number may be lower.
-- 'live' is still 100000 rows: SELECT count(*) FROM churn;

\echo '== 2. what the statistics collector believes'
SELECT n_live_tup, n_dead_tup, n_tup_upd, n_tup_hot_upd FROM pg_stat_user_tables WHERE relname = 'churn';
-- Expect: n_tup_upd = 300000. n_dead_tup is large (up to 300000; less if pruning already removed some).
-- These counters are reported by the backends with a delay: if you see zeros, wait a few seconds and run the
-- query again as a NEW statement outside any open transaction (values are cached until the end of a transaction).

\echo '== 3. VACUUM: dead versions are marked reusable'
VACUUM (VERBOSE) churn;
-- Expect, in the INFO output, a line of the form
--   tuples: N removed, 100000 remain, 0 are dead but not yet removable
-- with N = the number of dead versions that pruning had not already removed, and 0 at the end because no old
-- transaction holds the horizon back.
SELECT pg_size_pretty(pg_relation_size('churn')) AS heap_after_vacuum;
-- Expect: about the same size as heap_after_updates. Plain VACUUM does not shrink the file (only free pages at the
-- very end can be truncated); it makes the space reusable.

\echo '== 4. the freed space is reused'
UPDATE churn SET v = v + 1;
SELECT pg_size_pretty(pg_relation_size('churn')) AS heap_after_one_more_update;
-- Expect: roughly the same size as heap_after_vacuum: the new versions go into the space VACUUM freed.

\echo '== 5. VACUUM FULL rewrites the table'
VACUUM (FULL, VERBOSE) churn;
SELECT pg_size_pretty(pg_relation_size('churn'))      AS heap_after_full,
       pg_size_pretty(pg_relation_size('churn_pkey')) AS pkey_after_full;
-- Expect: back to about the size of step 0. VACUUM FULL takes an ACCESS EXCLUSIVE lock for the whole rewrite:
-- do not run it on a table you cannot afford to block.

-- ---------------------------------------------------------------------------------------------
-- Two sessions: a long transaction keeps the horizon back. (Remove the comment marker when pasting.)
-- ---------------------------------------------------------------------------------------------
-- [A] BEGIN ISOLATION LEVEL REPEATABLE READ;
-- [A] SELECT count(*) FROM churn;                    -- takes the snapshot; leave this session open and idle
-- [B] UPDATE churn SET v = v + 1;                    -- 100000 new versions; A can still see the old ones
-- [B] VACUUM (VERBOSE) churn;
--     Expect: "... 100000 are dead but not yet removable" (about; the old versions A may still need) and the
--     same dead versions still there after the VACUUM.
-- [B] SELECT pid, state, backend_xmin FROM pg_stat_activity WHERE backend_xmin IS NOT NULL;
--     Expect: A's row, with the transaction-id horizon that blocks the cleanup.
-- [A] COMMIT;
-- [B] VACUUM (VERBOSE) churn;
--     Expect: the same versions are now removed ("... 100000 removed ... 0 are dead but not yet removable").
