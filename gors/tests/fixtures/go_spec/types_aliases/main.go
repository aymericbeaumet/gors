package main

type Count = int

func main() {
	var count Count = 6
	if count != 6 {
		panic("alias value changed")
	}
}
