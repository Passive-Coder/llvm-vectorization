// Small standalone program for the TUI's Run program comparison.
// Declaring puts directly keeps LLVM IR generation independent of SDK headers.
extern int puts(const char *);

__attribute__((noinline))
void add_arrays(const int *restrict a, const int *restrict b,
                int *restrict out, unsigned count) {
  for (unsigned i = 0; i < count; ++i)
    out[i] = a[i] + b[i];
}

int main(void) {
  int a[1024], b[1024], out[1024];
  for (unsigned i = 0; i < 1024; ++i) {
    a[i] = (int)i;
    b[i] = (int)(2 * i);
  }
  add_arrays(a, b, out, 1024);
  int sum = 0;
  for (unsigned i = 0; i < 1024; ++i)
    sum += out[i];
  if (sum != 1571328)
    return 1;
  puts("sum=1571328");
  return 0;
}
