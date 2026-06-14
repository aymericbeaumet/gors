package main

type Point struct {
	X int
	Y int
}

type Line struct {
	Points []Point
}

func main() {
	primes := []int{2, 3, 5, 7, 11}
	vowels := [128]bool{'a': true, 'e': true, 'i': true, 'o': true, 'u': true}
	filter := [10]float64{-1, 4: -0.1, -0.1, 9: -1}
	days := [...]string{"Sat", "Sun"}
	line := Line{Points: []Point{{1, 2}, {X: 3, Y: 4}}}
	table := map[string]Point{"origin": {0, 0}, "next": {X: 5, Y: 6}}
	pointers := [2]*Point{{7, 8}, {}}
	left := &Point{1, 2}
	right := &Point{1, 2}

	if len(primes) != 5 || !vowels['e'] {
		panic("slice or keyed array literal changed")
	}
	if filter[0] != -1 || filter[4] != -0.1 || filter[5] != -0.1 || filter[9] != -1 {
		panic("indexed array literal changed")
	}
	if len(days) != 2 || days[1] != "Sun" {
		panic("ellipsis array literal changed")
	}
	if line.Points[1].Y != 4 || table["next"].X != 5 {
		panic("nested composite literal changed")
	}
	if pointers[0].Y != 8 || pointers[1].X != 0 {
		panic("pointer composite literal changed")
	}
	if left == right {
		panic("distinct composite literal pointers compared equal")
	}
}
