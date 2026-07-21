package main

type Counter struct {
	value int
}

func (counter Counter) Read() int {
	return counter.value
}

func (counter *Counter) Add(delta int) int {
	counter.value += delta
	return counter.value
}

type Reader interface {
	Read() int
}

func main() {
	value := Counter{value: 1}
	readSavedValue := value.Read
	value.value = 5

	pointer := &Counter{value: 2}
	addThroughSavedPointer := pointer.Add
	pointer.value = 4

	addressable := Counter{value: 7}
	addThroughImplicitAddress := addressable.Add

	var reader Reader = Counter{value: 11}
	readThroughInterface := reader.Read
	reader = Counter{value: 12}

	if readSavedValue() != 1 {
		panic("value receiver was not copied when method value was formed")
	}
	if addThroughSavedPointer(3) != 7 || pointer.value != 7 {
		panic("pointer receiver was not saved when method value was formed")
	}
	if addThroughImplicitAddress(2) != 9 || addressable.value != 9 {
		panic("addressable value did not bind a pointer-receiver method")
	}
	if readThroughInterface() != 11 {
		panic("interface receiver was not saved when method value was formed")
	}
	println("method-value-receiver-capture: ok")
}
