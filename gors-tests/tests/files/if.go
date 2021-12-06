package main

func main() {
	if x > max {
		x = max
	} else {
		x = 0
	}

	if x := f(); x < y {
		return x
	} else if x > z {
		return z
	} else {
		return y
	}
}
