package main

func main() {
	r := (<-chan int)(make(chan int))
	// illegal: a directional channel cannot be converted back to a
	// bidirectional channel.
	c := (chan int)(r)
	_ = c
}
