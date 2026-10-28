void add_arrays(const float *a, const float *b, float *result, int count) {
    for (int i = 0; i < count; ++i) {
        result[i] = a[i] + b[i];
    }
}
