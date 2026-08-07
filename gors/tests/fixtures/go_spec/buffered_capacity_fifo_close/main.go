package main

func main() {
	ch := make(chan string, 3)
	println("start", len(ch), cap(ch))
	ch <- "a"
	ch <- "b"
	ch <- "c" // fills the buffer exactly; must not block
	println("full", len(ch), cap(ch))
	println("recv", <-ch) // FIFO: first value sent comes out first
	ch <- "d"             // room again after one receive
	close(ch)
	println("closed-len", len(ch), cap(ch))
	// A closed channel delivers the remaining queued values in order...
	v1, ok1 := <-ch
	v2, ok2 := <-ch
	v3, ok3 := <-ch
	println("queued", v1, ok1, v2, ok2, v3, ok3)
	// ...then yields zero values with ok=false forever.
	for i := 0; i < 2; i++ {
		v, ok := <-ch
		println("drained", v == "", ok, len(ch))
	}
}
