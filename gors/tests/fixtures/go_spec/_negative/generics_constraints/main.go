package main

func Identity[T int | string](value T) T {
	return value
}

func main() {
	_ = Identity(1.5)
}
