package main

func main() {
	x := 1
Label1:
	for i := 0; i < 1; i++ {
		x := 2
		if x != 2 {
			panic("labeled block shadowing failed")
		}
		break Label1
	}
	if x != 1 {
		panic("outer labeled binding changed")
	}
}
