package main

func main() {
	result := "main ok"
	if result != "main ok" {
		panic("main execution changed")
	}
}
