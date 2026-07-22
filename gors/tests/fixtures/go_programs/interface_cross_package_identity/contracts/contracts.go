package contracts

type Reader interface {
	Read() int
}

type Writer interface {
	Write(int)
}

func HasAnonymousWriter(reader Reader) bool {
	_, ok := reader.(interface {
		Write(int)
	})
	return ok
}
