package main

func sum(limit int) int {
	total := 0
	for index := 0; index < limit; index++ {
		if index == 2 {
			continue
		}
		total += index
	}
	return total
}

func main() {
	println(sum(5))
}
