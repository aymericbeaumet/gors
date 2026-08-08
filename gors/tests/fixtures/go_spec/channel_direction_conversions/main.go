package main

func main() {
	ch := make(chan int, 2)
	s := (chan<- int)(ch)  // explicit conversion to send-only
	r := (<-chan int)(ch)  // explicit conversion to receive-only
	var s2 chan<- int = ch // constrained by assignment
	s <- 1
	s2 <- 2
	println(len(s), cap(s), len(r), cap(r))
	a := <-r
	b := <-r
	close(s) // close accepts a send-only channel
	v, ok := <-r
	println(a, b, v, ok)

	// Associativity: chan<- <-chan int is chan<- (<-chan int).
	inner := make(chan int, 1)
	inner <- 42
	nested := make(chan (<-chan int), 1)
	var nestedSend chan<- <-chan int = nested
	nestedSend <- inner
	got := <-<-nested
	println(got)
}
