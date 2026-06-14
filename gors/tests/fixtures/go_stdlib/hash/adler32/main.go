package main

import (
	"hash/adler32"
)

func assert(ok bool, msg string) {
	if !ok {
		panic(msg)
	}
}

func main() {
	sum := adler32.Checksum([]byte("gors"))
	assert(adler32.Size == 4, "size")
	assert(sum != 0, "checksum")

	h := adler32.New()
	n, _ := h.Write([]byte("gors"))
	assert(n == 4, "write count")
	assert(h.Sum32() == sum, "sum32")
	assert(len(h.Sum(nil)) == adler32.Size, "sum length")
}
