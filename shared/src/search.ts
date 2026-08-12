/**
 * Search ranking constants — blend of text match quality and place size.
 * score = matchQuality + min(POP_BOOST_CAP, log10(1 + population) * POP_BOOST_WEIGHT)
 * matchQuality: exact 1.0, prefix 0.9, fts rank normalized below that.
 */
export const SEARCH = {
  MATCH_EXACT: 1.0,
  MATCH_PREFIX: 0.9,
  POP_BOOST_WEIGHT: 0.1,
  POP_BOOST_CAP: 0.8,
  MIN_QUERY_LEN: 2,
  MAX_QUERY_LEN: 80,
  DEFAULT_LIMIT: 10,
  MAX_LIMIT: 50,
} as const;
