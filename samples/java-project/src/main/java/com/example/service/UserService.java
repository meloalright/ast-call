package com.example.service;

import com.example.utils.StringUtils;

public class UserService {
    private String name;

    public UserService(String name) {
        this.name = name;
    }

    public String getDisplayName() {
        return StringUtils.capitalize(name);
    }

    public void printInfo() {
        String display = getDisplayName();
        System.out.println("User: " + display);
    }
}
