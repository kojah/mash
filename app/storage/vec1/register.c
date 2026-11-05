/* Register the bundled extension before opening any SQLite connections.
 * Native constructors keep application Rust free of unsafe FFI or dynamic
 * extension loading. SQLite owns per-connection initialization and errors.
 */
extern int sqlite3_vec1_extra_init(const char *);

static void mash_vec1_register(void) {
  (void)sqlite3_vec1_extra_init(0);
}

#ifdef _MSC_VER
#pragma section(".CRT$XCU", read)
__declspec(allocate(".CRT$XCU")) void (*mash_vec1_initializer)(void) =
    mash_vec1_register;
#else
__attribute__((constructor)) static void mash_vec1_initialize(void) {
  mash_vec1_register();
}
#endif
