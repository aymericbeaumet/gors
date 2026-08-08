package main

type idx int

func main() {
	a := [...]int{9: 1}
	println(len(a), a[0], a[9])

	b := [...]int{2: 20, 30, 0: 10}
	println(len(b), b[0], b[1], b[2], b[3])

	const k = 5
	c := [...]string{k: "five", "six"}
	println(len(c), c[0] == "", c[5], c[6])

	d := [...]int{idx(3): 42}
	println(len(d), d[3])

	s := []int{9: 90, 100}
	println(len(s), cap(s), s[9], s[10], s[4])

	sparse := []string{1: "one", 3: "three"}
	println(len(sparse), sparse[0] == "", sparse[1], sparse[2] == "", sparse[3])
}
