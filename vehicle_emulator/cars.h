/*
prologue
Name of program: cars.h
Description: Header file for cars.c, defines the struct, init, and print functions
Author: Saurav Renju / Alec Slavik
Date Created: 2/11/2026
Date Revised: 3/1/2026
Revision History: Included in the numerous sprint artifacts.
*/

#pragma once

#include <stdint.h>

typedef struct
{
    char vin[18];      // 17 chars + null terminator
    int year;
    char make[32];
    char model[32];

    // World-space position (meters)
    double x;
    double y;
} Car;

// Constructor-style initializer
void car_init(
    Car *car,
    const char *vin,
    int year,
    const char *make,
    const char *model,
    double start_x,
    double start_y
);

// Debug print
void car_print(const Car *car);

