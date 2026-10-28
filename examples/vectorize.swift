func add(_ a: [Float], _ b: [Float], _ count: Int) -> [Float] {
    var result = [Float](repeating: 0, count: count)
    for i in 0..<count {
        result[i] = a[i] + b[i]
    }
    return result
}
