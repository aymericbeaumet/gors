package helper

var initialized bool

func init() {
	initialized = true
}

func Value() string {
	if !initialized {
		return "uninitialized"
	}
	return "helper"
}
