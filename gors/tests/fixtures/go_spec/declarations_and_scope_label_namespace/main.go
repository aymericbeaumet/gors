package main

func sameNameInOtherFunction() int {
	// The label namespace is per-function: reusing the label name "loop"
	// declared in main is legal here.
	total := 0
loop:
	for i := 0; i < 3; i++ {
		if i == 2 {
			break loop
		}
		total += i + 1
	}
	return total
}

func main() {
	// A label does not conflict with a variable of the same name: "loop"
	// below names both a label and an int variable.
	loop := 0
loop:
	for i := 0; i < 5; i++ {
		if i == 1 {
			loop = 40
			continue loop
		}
		if i == 3 {
			break loop
		}
		loop++
	}
	println("variable loop", loop)
	println("other function", sameNameInOtherFunction())

	// Labels are function scoped, not block scoped: "done" is declared at
	// function depth but referenced from inside two nested blocks.
	steps := 0
	{
		{
			if steps == 0 {
				goto done
			}
		}
	}
	steps = 100
done:
	steps++
	println("steps", steps)
}
