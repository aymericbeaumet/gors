package testfiles

// Test basic labeled statement
func basicLabel() {
start:
	_ = 1
	goto start
}

// Test label with for loop
func labeledFor() int {
	count := 0
loop:
	for i := 0; i < 10; i++ {
		if i == 5 {
			break loop
		}
		count++
	}
	return count
}

// Test nested labeled loops
func nestedLabels() int {
	result := 0
outer:
	for i := 0; i < 3; i++ {
	inner:
		for j := 0; j < 3; j++ {
			if j == 2 {
				break inner
			}
			if i == 2 && j == 1 {
				break outer
			}
			result++
		}
	}
	return result
}

// Test labeled switch
func labeledSwitch(x int) int {
sw:
	switch x {
	case 1:
		return 1
	case 2:
		break sw
	case 3:
		return 3
	}
	return 0
}

// Test labeled select
func labeledSelect(ch chan int) {
sel:
	select {
	case <-ch:
		break sel
	default:
		return
	}
}

// Test multiple labels
func multipleLabels(x int) int {
	result := 0
first:
	result++
	if result < x {
		goto first
	}
second:
	result += 2
	if result < x*2 {
		goto second
	}
	return result
}

// Test label before block statement
func labelBeforeBlock() {
block:
	{
		_ = 1
		goto block
	}
}

// Test label before if statement
func labelBeforeIf(x int) int {
check:
	if x > 0 {
		x--
		goto check
	}
	return x
}
