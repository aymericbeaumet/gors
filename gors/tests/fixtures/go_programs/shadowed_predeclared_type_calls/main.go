package main

func main() {
	called := false
	{
		string := func(value string) string {
			if len(value) == 4 {
				return "string result"
			}
			return "unexpected"
		}
		called = string("call") == "string result"
	}

	if !called {
		zero := len("")
		_ = 1 / zero
	}
}
