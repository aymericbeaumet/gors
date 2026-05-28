package main

import "fmt"

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

	fmt.Println(
		len(primes),
		vowels['e'],
		filter[0],
		filter[4],
		filter[5],
		filter[9],
		len(days),
		days[1],
		line.Points[1].Y,
		table["next"].X,
		pointers[0].Y,
		pointers[1].X,
		left != right,
	)
}
