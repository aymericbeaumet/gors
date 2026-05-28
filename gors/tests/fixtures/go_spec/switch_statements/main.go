package main

import "fmt"

func classify(value int) string {
	switch doubled := value * 2; {
	case doubled < 0:
		return "negative"
	case doubled == 0:
		return "zero"
	default:
		return "positive"
	}
}

func tag(value int) string {
	switch value {
	case 1, 2:
		return "small"
	case 3:
		return "three"
	default:
		return "other"
	}
}

func main() {
	fmt.Println(classify(-1), classify(0), classify(3), tag(2), tag(3), tag(9))
}
