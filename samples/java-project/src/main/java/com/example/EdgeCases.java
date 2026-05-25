package com.example;

// Edge case: comment should not appear as a caller
// StringUtils.capitalize("this is a comment, not a call")

class EdgeCases {
    void commentedOutCall() {
    }

    // Edge case: same-name method should not appear as caller of StringUtils.capitalize
    String capitalize(String localArg) {
        return "local: " + localArg;
    }

    void callsLocalCapitalize() {
        capitalize("local only");
    }
}
