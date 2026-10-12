-- Lab P12a, PostgreSQL part 5: reading EXPLAIN (ANALYZE, BUFFERS); estimates versus reality; index-only scans.
-- Theory: docs/12-he-thong-du-lieu/15-postgresql-ben-trong.md (section 8) and 03-storage-engine.md (section 2, B-tree).
--
-- STATUS: NOT RUN (no Docker / psql was used by the author). "Expect" lines are predictions from the
-- PostgreSQL 16 documentation, not recorded output. Which plan the planner picks, and every timing,
-- depends on your machine, on PostgreSQL's cost settings and on the table statistics; the lab asks you to
-- compare the numbers inside ONE run, not to match mine.
--
-- Run (single session, a few seconds, creates about 100 MB):
--   docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/05_explain.sql

\echo '== 0. data: 1,000,000 rows; id is unique; user_id has 100,000 distinct values (10 rows each)'
DROP TABLE IF EXISTS events;
CREATE TABLE events (id int PRIMARY KEY, user_id int NOT NULL, kind text NOT NULL, payload text NOT NULL);
INSERT INTO events
SELECT g, ((g::bigint * 7919) % 100000)::int, CASE WHEN g % 1000 = 0 THEN 'purchase' ELSE 'view' END, repeat('x', 40)
FROM generate_series(1, 1000000) AS g;
ANALYZE events;
SET max_parallel_workers_per_gather = 0;      -- keep the plans free of Gather nodes
SELECT pg_size_pretty(pg_relation_size('events'))                                  AS heap,
       pg_relation_size('events') / current_setting('block_size')::int            AS heap_pages,
       pg_size_pretty(pg_relation_size('events_pkey'))                             AS pkey;

\echo '== 1. one row by primary key'
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM events WHERE id = 424242;
-- Expect: Index Scan using events_pkey; a handful of buffers (the index levels plus one heap page), not thousands.

\echo '== 2. a filter on a column without an index'
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM events WHERE user_id = 4242;
-- Expect: Seq Scan on events with Filter: (user_id = 4242) and "Rows Removed by Filter" of about 999990;
-- the Buffers count is about heap_pages from step 0 (every page is read to find 10 rows); the actual time is
-- orders of magnitude above step 1.

\echo '== 3. the same query after CREATE INDEX'
CREATE INDEX events_user_idx ON events (user_id);
ANALYZE events;
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM events WHERE user_id = 4242;
-- Expect: Index Scan or Bitmap Heap Scan over events_user_idx, about 10 rows (the planner's estimate comes from a
-- sample, so it may be a little off), tens of buffers at most, and a much shorter time than step 2.

\echo '== 4. an index does not mean the index is used: selectivity'
CREATE INDEX events_kind_idx ON events (kind);
ANALYZE events;
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM events WHERE kind = 'view';      -- 99.9% of the rows
EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM events WHERE kind = 'purchase';  -- 0.1% of the rows
-- Expect: the first one is a Seq Scan even though events_kind_idx exists (reading almost every row through an index
-- would cost more than reading the heap in order); the second one uses an index-based plan with rows close to 1000.

\echo '== 5. estimates versus reality: two correlated columns'
DROP TABLE IF EXISTS corr;
CREATE TABLE corr AS SELECT g AS id, g % 100 AS a, g % 100 AS b FROM generate_series(1, 100000) AS g;
ANALYZE corr;
EXPLAIN (ANALYZE) SELECT * FROM corr WHERE a = 1 AND b = 1;
-- Expect: estimated rows = 10 (100000 x 1/100 x 1/100: the planner treats the two conditions as independent)
-- but actual rows = 1000 (b is always equal to a).
CREATE STATISTICS corr_ab (dependencies) ON a, b FROM corr;
ANALYZE corr;
EXPLAIN (ANALYZE) SELECT * FROM corr WHERE a = 1 AND b = 1;
-- Expect: the estimate moves close to 1000.

\echo '== 6. EXPLAIN ANALYZE really executes the statement: wrap writes in a transaction and roll back'
BEGIN;
EXPLAIN (ANALYZE, BUFFERS) UPDATE events SET kind = 'view' WHERE id <= 1000;
ROLLBACK;
SELECT count(*) AS purchases_in_first_1000 FROM events WHERE id <= 1000 AND kind = 'purchase';
-- Expect: 1 (the row with id = 1000): the rolled-back UPDATE left the data as it was.

\echo '== 7. an index-only scan depends on the visibility map'
VACUUM (ANALYZE) events;
EXPLAIN (ANALYZE, BUFFERS) SELECT user_id FROM events WHERE user_id BETWEEN 4000 AND 4010;
-- Expect after VACUUM: Index Only Scan using events_user_idx with "Heap Fetches: 0" (or a very small number).
UPDATE events SET payload = payload WHERE user_id BETWEEN 4000 AND 4010;
EXPLAIN (ANALYZE, BUFFERS) SELECT user_id FROM events WHERE user_id BETWEEN 4000 AND 4010;
-- Expect: still an Index Only Scan, but now with Heap Fetches > 0: the pages that were just written are no longer
-- marked all-visible until the next VACUUM.
