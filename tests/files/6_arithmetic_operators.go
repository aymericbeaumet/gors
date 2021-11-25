package main

func main() {
	a := 0

	a = a + a
	a += a

	a = a - a
	a -= a

	a = a * a
	a *= a

	a = a / a
	a /= a

	a = a % a
	a %= a

	a = a & a
	a &= a

	a = a | a
	a |= a

	a = a ^ a
	a ^= a

	a = a &^ a
	a &^= a

	a = a << a
	a <<= a

	a = a >> a
	a >>= a
}
