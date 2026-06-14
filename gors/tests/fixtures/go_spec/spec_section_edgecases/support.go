package main

var packageNumber = helperValue()

func helperValue() int {
	return 19
}

func init() {
	packageNumber++
}
