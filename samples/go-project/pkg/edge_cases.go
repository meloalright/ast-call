package pkg

// Edge case: comment should not appear as a caller
// FormatOutput("this is a comment, not a call")

func commentedOutCall() {
}

// Edge case: same-name function should not appear as caller of utils.FormatOutput
func FormatOutput(localArg string) string {
	return "local: " + localArg
}

func callsLocalFormatOutput() {
	FormatOutput("local only")
}
