#include "cars.h"
#include <stdio.h>
#include <string.h>

/*
prologue
Name of program: cars.c
Description: Initialize a car structure, copies strings and sets starting positions
Author: Saurav Renju / Alec Slavik
Date Created: 2/11/2026
Date Revised: 3/1/2026
Revision History: Included in the numerous sprint artifacts.
*/

void car_init(
    Car *car,
    const char *vin,
    int year,
    const char *make,
    const char *model,
    double start_x,
    double start_y
)
{
    memset(car, 0, sizeof(*car));

    // VIN must be <= 17 chars
    strncpy(car->vin, vin, 17);
    car->vin[17] = '\0';

    car->year = year;

    strncpy(car->make, make, sizeof(car->make) - 1);
    strncpy(car->model, model, sizeof(car->model) - 1);

    car->x = start_x;
    car->y = start_y;
}

/*
Car print is used to check if cars are working (debugging), and printing they're provided values
*/

void car_print(const Car *car)
{
    printf("Car:\n");
    printf("  VIN:   %s\n", car->vin);
    printf("  Year:  %d\n", car->year);
    printf("  Make:  %s\n", car->make);
    printf("  Model: %s\n", car->model);
    printf("  Pos:   (%.2f, %.2f) meters\n", car->x, car->y);
}

#if defined(TEST_CAR_MAIN) || defined(test_car_main)

/*
Start main with a preset car
*/
int main(void)
{
    Car car;

    car_init(
        &car,
        "1HGBH41JXMN109186",
        2022,
        "Toyota",
        "Camry",
        10.0,   // x meters
        -5.0    // y meters
    );

    car_print(&car);
    return 0;
}
#endif

