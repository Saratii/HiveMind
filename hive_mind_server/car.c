#include "car.h"
#include <stdlib.h>
#include <string.h>

#define MAX_CARS 1000

static Car car_list[MAX_CARS];
static int car_count = 0;

void add_car(const char *license_plate, Point start, Point destination)
{
    if (car_count < MAX_CARS)
    {
        strncpy(car_list[car_count].license_plate, license_plate, 19);
        car_list[car_count].license_plate[19] = '\0';
        car_list[car_count].start = start;
        car_list[car_count].destination = destination;
        car_count++;
    }
}

int get_car_count(void)
{
    return car_count;
}