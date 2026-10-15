CREATE TABLE "t" AS SELECT * FROM (VALUES
  ('http://example.com/v/0', 42::BIGINT, 1.5::DOUBLE, 'infinity'::DOUBLE, true, DATE '2026-10-06',
    TIMESTAMP '2026-10-06 12:30:00', TIME '12:30:00', 1.5::DECIMAL(4,1), '\x01\xAB'::BLOB, 'x', 'http://example.com/o'),
  ('http://example.com/v/1', -1::BIGINT, '-infinity'::DOUBLE, 'nan'::DOUBLE, false, DATE '1999-01-31',
    TIMESTAMP '1999-01-31 00:00:00', TIME '00:00:00', -2.0::DECIMAL(4,1), ''::BLOB, 'y', 'http://example.com/p')
) AS v(subject, i, d, f, b, day, ts, tm, dec, bin, s, ref);
