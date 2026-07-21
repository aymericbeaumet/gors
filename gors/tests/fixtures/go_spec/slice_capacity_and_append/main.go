package main

func main() {
	base := make([]int, 2, 4)
	base[0] = 1
	base[1] = 2

	shared := base[:1]
	shared = append(shared, 9)

	limited := base[:1:1]
	limited = append(limited, 7)
	limited[0] = 8

	extended := base[:3]

	if len(shared) != 2 || cap(shared) != 4 || base[1] != 9 {
		panic("append within capacity did not reuse the underlying array")
	}
	if len(limited) != 2 || cap(limited) < 2 || limited[0] != 8 || limited[1] != 7 {
		panic("append after a full slice expression changed")
	}
	if base[0] != 1 {
		panic("append past a full slice capacity reused the original array")
	}
	if extended[2] != 0 {
		panic("reslicing within capacity did not expose the underlying zero value")
	}
	println("slice-capacity-and-append: ok")
}
