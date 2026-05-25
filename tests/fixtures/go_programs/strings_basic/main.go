package main

import (
	"fmt"
	"strings"
)

func main() {
	fmt.Println(strings.Contains("Hello, World", "World"))
	fmt.Println(strings.HasPrefix("Hello", "He"))
	fmt.Println(strings.HasSuffix("Hello", "lo"))
	fmt.Println(strings.ToUpper("hello"))
	fmt.Println(strings.ToLower("HELLO"))
	fmt.Println(strings.TrimSpace("  hello  "))
	fmt.Println(strings.Repeat("ab", 3))
	fmt.Println(strings.Count("hello", "l"))
	fmt.Println(strings.ReplaceAll("hello world", "world", "Go"))
	fmt.Println(strings.Index("hello", "ll"))
}
