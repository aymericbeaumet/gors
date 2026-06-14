package main

type Score int
type Ratio float64

func main() {
	score := Score(4)
	score += Score(3)
	score = score*Score(2) - Score(1)
	score ^= Score(3)
	score <<= 1
	score >>= 2
	score |= Score(8)
	score &= Score(14)
	score &^= Score(4)
	score %= Score(6)

	ratio := Ratio(2.5)
	ratio += Ratio(1.5)
	ratio = ratio / Ratio(2.0)

	if int(score) != 4 || score > Score(10) {
		panic("named integer operations changed")
	}
	if float64(ratio) != 2.0 || ratio != Ratio(2.0) {
		panic("named float operations changed")
	}
}
