package testfiles

// Test expression switch statement
func expressionSwitch(x int) int {
	switch x {
	case 1:
		return 1
	case 2, 3:
		return 2
	default:
		return 0
	}
}

// Test expression switch with init statement
func expressionSwitchWithInit(y int) int {
	switch x := y * 2; x {
	case 2:
		return 1
	case 4:
		return 2
	default:
		return 0
	}
}

// Test expression switch without tag
func expressionSwitchNoTag(x int) string {
	switch {
	case x < 0:
		return "negative"
	case x == 0:
		return "zero"
	case x > 0:
		return "positive"
	default:
		return "unknown"
	}
}

// Test type switch statement
func typeSwitch(x interface{}) string {
	switch x.(type) {
	case int:
		return "int"
	case string:
		return "string"
	case bool:
		return "bool"
	default:
		return "unknown"
	}
}

// Test type switch with assignment
func typeSwitchWithAssign(x interface{}) string {
	switch v := x.(type) {
	case int:
		_ = v
		return "int"
	case string:
		_ = v
		return "string"
	default:
		return "unknown"
	}
}

// Test type switch with init statement
func typeSwitchWithInit(x interface{}) string {
	switch y := x; v := y.(type) {
	case int:
		_ = v
		return "int"
	default:
		return "unknown"
	}
}

// Test empty switch
func emptySwitch(x int) {
	switch x {
	}
}

// Test switch with fallthrough
func switchFallthrough(x int) int {
	result := 0
	switch x {
	case 1:
		result = 1
		fallthrough
	case 2:
		result += 2
	default:
		result = -1
	}
	return result
}
