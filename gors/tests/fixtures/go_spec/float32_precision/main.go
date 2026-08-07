package main

func main() {
	big := float32(1 << 24)
	bumped := big + 1
	big64 := float64(1 << 24)
	bumped64 := big64 + 1
	if bumped != big {
		panic("float32 addition did not round at 24 bits of precision")
	}
	if bumped64 == big64 {
		panic("float64 addition lost precision it should keep")
	}
	tenth := float32(0.1)
	if float64(tenth) == 0.1 {
		panic("float32 rounding matched float64")
	}
	odd := float32(16777215)
	stepped := odd + 2
	if stepped != 16777216 {
		panic("float32 round-to-nearest-even changed")
	}
	var small float32 = 1
	for i := 0; i < 30; i++ {
		small /= 2
	}
	if small == 0 {
		panic("float32 division underflowed unexpectedly")
	}
	println(bumped == big, bumped64 == big64, stepped == 16777216, float64(tenth) > 0.1)
}
