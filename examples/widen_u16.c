// Standalone demo for the vectorizer TUI. These compiler-provided type widths
// avoid SDK headers on machines with an incomplete clang sysroot.
typedef __SIZE_TYPE__ size_t;
typedef unsigned short uint16_t;
typedef int int32_t;
_Static_assert(sizeof(uint16_t) == 2, "uint16_t must be 16 bits");
_Static_assert(sizeof(int32_t) == 4, "int32_t must be 32 bits");
extern int puts(const char *);

__attribute__((noinline))
void widen_u16(int32_t *restrict out, const uint16_t *restrict input,
               size_t n) {
  for (size_t i = 0; i < n; ++i)
    out[i] = (int32_t)input[i] + 7;
}

int main(void) {
  enum { ELEMENTS = 8192, REPEATS = 20000 };
  uint16_t input[ELEMENTS];
  int32_t output[ELEMENTS];
  for (size_t i = 0; i < ELEMENTS; ++i)
    input[i] = (uint16_t)(i * 3);
  for (int repeat = 0; repeat < REPEATS; ++repeat)
    widen_u16(output, input, ELEMENTS);
  for (size_t i = 0; i < ELEMENTS; ++i)
    if (output[i] != (int32_t)input[i] + 7)
      return 1;
  puts("widen_u16: ok");
  return 0;
}
