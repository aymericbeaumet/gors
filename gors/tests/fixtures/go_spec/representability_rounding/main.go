package main

func main() {
	const a float32 = 16777217          // 2^24+1: tie, rounds to even -> 16777216
	const b float32 = 16777219          // tie, rounds to even -> 16777220
	const c float64 = 1<<53 + 1         // tie, rounds to even -> 2^53
	const e float32 = 2.718281828459045 // spec example: rounds to 2.7182817
	var by byte = 42.0                  // float constant with integral value is representable by byte
	var u uint64 = 1e10                 // 1e10 is in the set of uint64 values
	if a != 16777216 || b != 16777220 || c != 9007199254740992 || e != 2.7182817 || by != 42 || u != 10000000000 {
		panic("representability rounding changed")
	}
	println(a == 16777216, b == 16777220, c == 1<<53, by, u)
	println(e)
}
