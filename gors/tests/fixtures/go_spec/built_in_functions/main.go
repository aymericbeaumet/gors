package main

func main() {
	values := make([]int, 2, 4)
	values[0] = 1
	values[1] = 2
	values = append(values, 3)
	clone := make([]int, len(values))
	copy(clone, values)
	mapping := map[string]int{"x": 1, "y": 2}
	mapLen := len(mapping)
	delete(mapping, "x")
	clear(mapping)
	pointer := new(int)
	*pointer = max(3, min(4, 5))
	complexValue := complex(1, 2)
	array := [3]int{1, 2, 3}
	text := "go"
	var channel chan int = make(chan int, 1)
	channel <- len(array)
	channelLen := len(channel)
	close(channel)
	received, ok := <-channel
	_, closedOk := <-channel
	print("ignored")
	println("ignored", 1)
	if len(values) != 3 || cap(values) != 4 || clone[2] != 3 {
		panic("slice builtins changed")
	}
	if *pointer != 4 {
		panic("new, min, or max builtins changed")
	}
	if real(complexValue) != 1 || imag(complexValue) != 2 {
		panic("complex builtin changed")
	}
	if len(text) != 2 || mapLen != 2 {
		panic("string or map len changed")
	}
	if channelLen != 1 || received != 3 || !ok || closedOk {
		panic("channel builtins changed")
	}
}
