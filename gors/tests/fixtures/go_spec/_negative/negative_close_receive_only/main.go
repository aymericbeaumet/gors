package main

func main() {
	ch := make(chan int, 1)
	var recvOnly <-chan int = ch
	close(recvOnly)
}
