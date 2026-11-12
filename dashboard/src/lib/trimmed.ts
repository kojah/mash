/**
 * A fixture for a test: an answer cut down to the fields the code under test
 * reads. The contract's types describe whole answers, which would bury what a
 * test is about.
 */
export function trimmed<T>(value: unknown): T {
  return value as T;
}
