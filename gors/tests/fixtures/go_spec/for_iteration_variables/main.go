package main

func main() {
	var clauseClosures []func() int
	for index := 0; index < 3; index++ {
		clauseClosures = append(clauseClosures, func() int {
			return index
		})
	}

	var rangeClosures []func() int
	for _, value := range []int{4, 5, 6} {
		rangeClosures = append(rangeClosures, func() int {
			return value
		})
	}

	if clauseClosures[0]() != 0 || clauseClosures[1]() != 1 || clauseClosures[2]() != 2 {
		panic("for-clause iteration variables were shared")
	}
	if rangeClosures[0]() != 4 || rangeClosures[1]() != 5 || rangeClosures[2]() != 6 {
		panic("range iteration variables were shared")
	}
	println("for-iteration-variables: ok")
}
