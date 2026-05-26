package main

func main() {
	counts := map[string]int{"seen": 1}
	counts["seen"]++
	counts["missing"]++
	counts["seen"]--
	println(counts["seen"], counts["missing"])
}
