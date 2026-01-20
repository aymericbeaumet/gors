package main

func main() {
	go Server()

	f := func(x, y int) int { return x + y }
	go f(0, 1)

	go func(ch chan<- bool) {
		sleep(10)
		ch <- true
	}(c)
}
