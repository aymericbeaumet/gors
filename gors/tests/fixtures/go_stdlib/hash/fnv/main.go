package main

import "hash/fnv"

func assert(ok bool, msg string) {
	if !ok {
		panic(msg)
	}
}

func main() {
	data := []byte("gors")

	h32 := fnv.New32()
	n, _ := h32.Write(data)
	assert(n == 4, "write32")
	assert(h32.Sum32() != 0, "sum32")
	assert(len(h32.Sum(nil)) == 4, "sum32 bytes")

	h32a := fnv.New32a()
	n, _ = h32a.Write(data)
	assert(n == 4, "write32a")
	assert(h32a.Sum32() != h32.Sum32(), "sum32a")

	h64 := fnv.New64()
	n, _ = h64.Write(data)
	assert(n == 4, "write64")
	assert(h64.Sum64() != 0, "sum64")
	assert(len(h64.Sum(nil)) == 8, "sum64 bytes")

	h64a := fnv.New64a()
	n, _ = h64a.Write(data)
	assert(n == 4, "write64a")
	assert(h64a.Sum64() != h64.Sum64(), "sum64a")

}
