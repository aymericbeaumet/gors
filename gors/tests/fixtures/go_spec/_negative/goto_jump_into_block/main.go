package main

func main() {
	n := 3
	if n%2 == 1 {
		goto L1
	}
	for n > 0 {
		n--
	L1:
		n--
	}
}
