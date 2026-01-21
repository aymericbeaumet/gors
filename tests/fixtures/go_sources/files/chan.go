package main

func main() {
	a := make(chan int)

	a <- a
	a(<-a)

	a <- <-a
	a <- (<-a)

	<-a <- a
	<-a(<-a)
}
