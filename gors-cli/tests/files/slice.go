package main

import "fmt"

func main() {
	var a []string

	primes := []int{2, 3, 5, 7, 11, 13}

	var s []int = primes[1:4]
	var s []int = primes[:4]
	var s []int = primes[1:]
	var s []int = primes[:]
	fmt.Println(s)
}
