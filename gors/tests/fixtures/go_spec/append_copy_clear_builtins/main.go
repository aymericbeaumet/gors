package main

func main() {
	bytes := []byte{'g', 'o'}
	bytes = append(bytes, "rs"...)
	more := []byte{'!', '?'}
	bytes = append(bytes, more...)

	dst := []byte{0, 0, 0, 0, 0}
	copied := copy(dst, "hello!")

	values := []int{1, 2, 3, 4}
	clear(values[1:3])

	if string(bytes) != "gors!?" {
		panic("append result changed")
	}
	if copied != 5 || string(dst) != "hello" {
		panic("copy result changed")
	}
	if values[0] != 1 || values[1] != 0 || values[2] != 0 || values[3] != 4 {
		panic("clear result changed")
	}
}
