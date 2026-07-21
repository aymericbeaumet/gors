package main

const (
	highBit = 1 << 255
	folded  = ((1 << 200) + (1 << 199)) >> 190
	lowBits = (highBit - 1) & 0xffff
)

func main() {
	if folded != 1536 || lowBits != 65535 {
		panic("large constant precision changed")
	}
	println("large-constant-precision: ok")
}
