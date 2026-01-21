package main

type s1 struct{}

type s2 struct {
	s1
	a    bool
	b, c bool
}

type s3 struct {
	*s2
	a    bool `hello:"world"`
	b, c bool `hello:"world"`
}

var v1 struct{}

var v2 struct {
	s1
	a    bool
	b, c bool
}

var v3 struct {
	*s2
	a    bool `hello:"world"`
	b, c bool `hello:"world"`
}
