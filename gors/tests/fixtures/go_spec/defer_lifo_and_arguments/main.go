package main

func deferredResult() (result int) {
	value := 1
	defer func(saved int) {
		result = result*10 + saved
	}(value)

	value = 2
	defer func(saved int) {
		result = result*10 + saved
	}(value)

	result = 3
	return
}

func main() {
	if deferredResult() != 321 {
		panic("defer order, saved arguments, or named result changed")
	}
	println("defer-lifo-and-arguments: ok")
}
