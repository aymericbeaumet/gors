package main

func main() {
	s := []int{0, 1, 2, 3, 4, 5}
	n2 := copy(s, s[2:])
	if n2 != 4 || s[0] != 2 || s[1] != 3 || s[2] != 4 || s[3] != 5 || s[4] != 4 || s[5] != 5 {
		panic("overlapping copy toward the front changed")
	}

	t := []int{0, 1, 2, 3, 4, 5}
	n := copy(t[2:], t[:4])
	if n != 4 || t[2] != 0 || t[3] != 1 || t[4] != 2 || t[5] != 3 {
		panic("overlapping copy toward the back changed")
	}

	dst := []int{9, 9, 9, 9}
	src := []int{1, 2}
	if copy(dst, src) != 2 || dst[0] != 1 || dst[1] != 2 || dst[2] != 9 {
		panic("copy count with short source changed")
	}

	b := make([]byte, 6)
	b[5] = 'x'
	if copy(b, "go") != 2 || b[0] != 'g' || b[1] != 'o' || b[2] != 0 || b[5] != 'x' {
		panic("copy from short string changed")
	}

	if copy(dst, []int(nil)) != 0 || copy([]int(nil), dst) != 0 {
		panic("copy involving nil slices changed")
	}

	println("copy-overlapping-and-counts: ok")
}
