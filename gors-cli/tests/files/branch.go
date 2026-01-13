package testfiles

// Test break statement
func testBreak() int {
	result := 0
	for i := 0; i < 10; i++ {
		if i == 5 {
			break
		}
		result++
	}
	return result
}

// Test continue statement
func testContinue() int {
	result := 0
	for i := 0; i < 10; i++ {
		if i%2 == 0 {
			continue
		}
		result++
	}
	return result
}

// Test break with label
func testBreakWithLabel() int {
	result := 0
outer:
	for i := 0; i < 5; i++ {
		for j := 0; j < 5; j++ {
			if i*j > 10 {
				break outer
			}
			result++
		}
	}
	return result
}

// Test continue with label
func testContinueWithLabel() int {
	result := 0
outer:
	for i := 0; i < 5; i++ {
		for j := 0; j < 5; j++ {
			if j == 2 {
				continue outer
			}
			result++
		}
	}
	return result
}

// Test goto statement
func testGoto(x int) int {
	result := 0
	if x > 0 {
		goto positive
	}
	result = -1
	goto done
positive:
	result = 1
done:
	return result
}

// Test fallthrough in switch
func testFallthrough(x int) int {
	result := 0
	switch x {
	case 1:
		result = 1
		fallthrough
	case 2:
		result += 10
	case 3:
		result = 3
	}
	return result
}

// Test nested break
func testNestedBreak() int {
	result := 0
	for i := 0; i < 3; i++ {
		for j := 0; j < 3; j++ {
			if j == 1 {
				break
			}
			result++
		}
	}
	return result
}

// Test break in switch inside for
func testBreakInSwitch() int {
	result := 0
	for i := 0; i < 5; i++ {
		switch i {
		case 3:
			break
		default:
			result++
		}
	}
	return result
}
