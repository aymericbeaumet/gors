package main

func makeArray(evaluations *int) [4]int {
	*evaluations = *evaluations + 1
	return [4]int{1, 2, 3, 4}
}

func main() {
	const ptrLen = len((*[7]int)(nil))
	const ptrCap = cap((*[7]int)(nil))
	if ptrLen != 7 || ptrCap != 7 {
		panic("len/cap of nil *[7]int changed")
	}

	const litLen = len([10]float64{imag(2i)})
	if litLen != 10 {
		panic("constant len of array literal changed")
	}

	evaluations := 0
	if len(makeArray(&evaluations)) != 4 || cap(makeArray(&evaluations)) != 4 {
		panic("len/cap of array-valued call changed")
	}
	if evaluations != 2 {
		panic("array-valued operand of len/cap was not evaluated")
	}

	var nilSlice []int
	var nilMap map[int]string
	var nilChan chan int
	if len(nilSlice) != 0 || cap(nilSlice) != 0 {
		panic("nil slice len/cap changed")
	}
	if len(nilMap) != 0 {
		panic("nil map len changed")
	}
	if len(nilChan) != 0 || cap(nilChan) != 0 {
		panic("nil channel len/cap changed")
	}

	const strLen = len("héllo")
	if strLen != 6 {
		panic("constant len of string changed")
	}
	println("len-cap-constant-and-nil: ok")
}
