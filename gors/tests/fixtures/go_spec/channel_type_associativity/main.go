package main

func main() {
	inner := make(chan int, 1)
	inner <- 5

	// The <- operator associates with the leftmost chan possible:
	// chan<- <-chan int  is  chan<- (<-chan int)
	// <-chan <-chan int  is  <-chan (<-chan int)
	wrap := make(chan (<-chan int), 1)
	var sendSide chan<- <-chan int = wrap
	sendSide <- inner
	var recvSide <-chan <-chan int = wrap
	got := <-<-recvSide
	println("nested", got)

	// chan<- chan int  is  chan<- (chan int)
	wrap2 := make(chan chan int, 1)
	var sendSide2 chan<- chan int = wrap2
	bidi := make(chan int, 1)
	bidi <- 6
	sendSide2 <- bidi
	println("nested2", <-<-wrap2)

	// A channel may be constrained by explicit conversion.
	base := make(chan int, 1)
	(chan<- int)(base) <- 7
	recvOnly := (<-chan int)(base)
	println("converted", <-recvOnly)

	// make can create a directional channel type directly.
	sendOnly := make(chan<- int, 1)
	sendOnly <- 8
	println("sendonly", len(sendOnly), cap(sendOnly))
}
