package main

func recv[C ~chan E | ~<-chan E, E any](channel C) E { return <-channel }

func main() {
	channel := make(chan int, 2)
	channel <- 5
	channel <- 6
	if recv(channel) != 5 {
		panic("bidirectional channel inference changed")
	}
	var receiveOnly <-chan int = channel
	if recv(receiveOnly) != 6 {
		panic("receive-only channel inference changed")
	}
	strings := make(chan string, 1)
	strings <- "s"
	var value string = recv(strings)
	if value != "s" {
		panic("channel element inference changed")
	}
	println("generics-channel-constraint-unification: ok")
}
