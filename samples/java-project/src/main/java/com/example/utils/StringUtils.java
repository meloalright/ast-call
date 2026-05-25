package com.example.utils;

public class StringUtils {
    public static String capitalize(String s) {
        if (s == null || s.isEmpty()) {
            return s;
        }
        return Character.toUpperCase(s.charAt(0)) + s.substring(1);
    }

    public static String reverse(String s) {
        return new StringBuilder(s).reverse().toString();
    }
}
