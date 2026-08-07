package main

const f = int32(1) << 33 // illegal: constant 8589934592 overflows int32

func main() {}
