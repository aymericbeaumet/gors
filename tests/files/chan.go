package main

func main() {
	a := make(chan int)
	b := make(chan<- float64)

	a <- a
	a(<-a)

	a <- <-a
	a <- (<-a)

	<-a <- a
	<-a(<-a)
}
