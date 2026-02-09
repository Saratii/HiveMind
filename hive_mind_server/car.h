#ifndef CAR_H
#define CAR_H

typedef struct
{
    double x;
    double y;
} Point;

typedef struct
{
    char license_plate[20];
    Point start;
    Point destination;
} Car;

void add_car(const char *license_plate, Point start, Point destination);
int get_car_count(void);

#endif