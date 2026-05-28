package main

import "fmt"

func main() {
	bytes := []byte{'g', 'o'}
	bytes = append(bytes, "rs"...)
	more := []byte{'!', '?'}
	bytes = append(bytes, more...)

	dst := []byte{0, 0, 0, 0, 0}
	copied := copy(dst, "hello!")

	values := []int{1, 2, 3, 4}
	clear(values[1:3])

	fmt.Println(string(bytes), copied, string(dst), values[0], values[1], values[2], values[3])
}
