#define _CRT_SECURE_NO_WARNINGS
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include "raylib.h"
#include "city_constants.h"

enum JsonTokenType
{
    JSON_TOKEN_UNDEFINED = 0,
    JSON_TOKEN_OBJECT = 1,
    JSON_TOKEN_ARRAY = 2,
    JSON_TOKEN_STRING = 3,
    JSON_TOKEN_PRIMITIVE = 4
};

struct JsonToken
{
    enum JsonTokenType type;
    int start;
    int end;
    int size;
    int parent;
};

struct JsonParser
{
    uint32_t position;
    uint32_t next_token_index;
    int32_t super_token_index;
};

struct Point2
{
    double x;
    double y;
};

struct RoadSegment
{
    int id;
    int point_count;
    struct Point2 *points;
};

struct CityMap
{
    struct RoadSegment *road_segments;
    int road_segment_count;
    int road_segment_capacity;
};

struct Camera2DState
{
    double offset_x;
    double offset_y;
    double zoom;
};

void city_map_initialize(struct CityMap *city_map)
{
    city_map->road_segments = NULL;
    city_map->road_segment_count = 0;
    city_map->road_segment_capacity = 0;
}

void city_map_free(struct CityMap *city_map)
{
    int segment_index = 0;
    for (segment_index = 0; segment_index < city_map->road_segment_count; segment_index++)
    {
        free(city_map->road_segments[segment_index].points);
    }
    free(city_map->road_segments);
    city_map_initialize(city_map);
}

void city_map_reserve_road_segments(struct CityMap *city_map, int required_capacity)
{
    if (city_map->road_segment_capacity >= required_capacity)
    {
        return;
    }
    int new_capacity = city_map->road_segment_capacity > 0 ? city_map->road_segment_capacity * 2 : 16;
    if (new_capacity < required_capacity)
    {
        new_capacity = required_capacity;
    }
    struct RoadSegment *new_segments = (struct RoadSegment *)realloc(
        city_map->road_segments,
        (size_t)new_capacity * sizeof(struct RoadSegment));
    if (new_segments == NULL)
    {
        fprintf(stderr, "out of memory\n");
        exit(1);
    }
    city_map->road_segments = new_segments;
    city_map->road_segment_capacity = new_capacity;
}

int city_map_add_road_segment(struct CityMap *city_map, int segment_id, struct Point2 *points, int point_count)
{
    if (point_count < 2)
    {
        return 0;
    }

    int point_index = 1;
    for (point_index = 1; point_index < point_count; point_index++)
    {
        double delta_x = points[point_index].x - points[point_index - 1].x;
        double delta_y = points[point_index].y - points[point_index - 1].y;
        int is_horizontal = (delta_y == 0.0 && delta_x != 0.0);
        int is_vertical = (delta_x == 0.0 && delta_y != 0.0);
        if (!(is_horizontal || is_vertical))
        {
            return 0;
        }
    }

    city_map_reserve_road_segments(city_map, city_map->road_segment_count + 1);

    struct RoadSegment *road_segment = &city_map->road_segments[city_map->road_segment_count++];
    road_segment->id = segment_id;
    road_segment->point_count = point_count;
    road_segment->points = (struct Point2 *)malloc((size_t)point_count * sizeof(struct Point2));
    if (road_segment->points == NULL)
    {
        fprintf(stderr, "out of memory\n");
        exit(1);
    }
    memcpy(road_segment->points, points, (size_t)point_count * sizeof(struct Point2));
    return 1;
}

uint32_t random_next_u32(uint64_t *state)
{
    *state = (*state * 6364136223846793005ULL) + 1442695040888963407ULL;
    uint32_t high = (uint32_t)(*state >> 32);
    uint32_t low = (uint32_t)(*state & 0xffffffffu);
    return high ^ low;
}

int random_range_int(uint64_t *state, int inclusive_minimum, int inclusive_maximum)
{
    uint32_t random_value = random_next_u32(state);
    int span = inclusive_maximum - inclusive_minimum + 1;
    return inclusive_minimum + (int)(random_value % (uint32_t)span);
}

void city_map_compute_bounds(
    struct CityMap *city_map,
    double *minimum_x,
    double *minimum_y,
    double *maximum_x,
    double *maximum_y)
{
    if (city_map->road_segment_count == 0)
    {
        *minimum_x = 0.0;
        *minimum_y = 0.0;
        *maximum_x = 1.0;
        *maximum_y = 1.0;
        return;
    }

    double min_x = city_map->road_segments[0].points[0].x;
    double min_y = city_map->road_segments[0].points[0].y;
    double max_x = min_x;
    double max_y = min_y;

    int segment_index = 0;
    for (segment_index = 0; segment_index < city_map->road_segment_count; segment_index++)
    {
        struct RoadSegment *segment = &city_map->road_segments[segment_index];
        int point_index = 0;
        for (point_index = 0; point_index < segment->point_count; point_index++)
        {
            double x = segment->points[point_index].x;
            double y = segment->points[point_index].y;
            if (x < min_x)
                min_x = x;
            if (y < min_y)
                min_y = y;
            if (x > max_x)
                max_x = x;
            if (y > max_y)
                max_y = y;
        }
    }

    *minimum_x = min_x;
    *minimum_y = min_y;
    *maximum_x = max_x;
    *maximum_y = max_y;
}

void city_map_add_rect_loop(struct CityMap *city_map, int segment_id, double left, double bottom, double right, double top)
{
    struct Point2 points[5];
    points[0].x = left;
    points[0].y = bottom;
    points[1].x = right;
    points[1].y = bottom;
    points[2].x = right;
    points[2].y = top;
    points[3].x = left;
    points[3].y = top;
    points[4].x = left;
    points[4].y = bottom;
    city_map_add_road_segment(city_map, segment_id, points, 5);
}

void city_map_generate_basic_city(struct CityMap *city_map, uint64_t seed)
{
    city_map_free(city_map);
    city_map_initialize(city_map);

    uint64_t random_state = seed != 0 ? seed : 1ULL;
    int next_segment_id = 1;

    int major_columns = 10 + (int)(random_next_u32(&random_state) % 6u);
    int major_rows = 10 + (int)(random_next_u32(&random_state) % 6u);

    double spacing_x = city_block_size_meters();
    double spacing_y = city_block_size_meters();
    double total_width = (major_columns - 1) * spacing_x;
    double total_height = (major_rows - 1) * spacing_y;
    double origin_x = -0.5 * total_width;
    double origin_y = -0.5 * total_height;

    int arterial_step = 4 + (int)(random_next_u32(&random_state) % 2u);
    int row_index = 0;
    for (row_index = 0; row_index < major_rows; row_index++)
    {
        int is_arterial = (row_index % arterial_step) == 0 || row_index == major_rows / 2;
        if (is_arterial)
        {
            struct Point2 points[2];
            points[0].x = origin_x + 0.0;
            points[0].y = origin_y + row_index * spacing_y;
            points[1].x = origin_x + total_width;
            points[1].y = origin_y + row_index * spacing_y;
            city_map_add_road_segment(city_map, next_segment_id++, points, 2);
        }
        else
        {
            int segment_count = 1 + (int)(random_next_u32(&random_state) % 3u);
            int segment_index = 0;
            for (segment_index = 0; segment_index < segment_count; segment_index++)
            {
                int left_column = random_range_int(&random_state, 0, major_columns - 2);
                int right_column = random_range_int(&random_state, left_column + 1, major_columns - 1);

                struct Point2 points[2];
                points[0].x = origin_x + left_column * spacing_x;
                points[0].y = origin_y + row_index * spacing_y;
                points[1].x = origin_x + right_column * spacing_x;
                points[1].y = origin_y + row_index * spacing_y;
                city_map_add_road_segment(city_map, next_segment_id++, points, 2);
            }
        }
    }

    int column_index = 0;
    for (column_index = 0; column_index < major_columns; column_index++)
    {
        int is_arterial = (column_index % arterial_step) == 0 || column_index == major_columns / 2;
        if (is_arterial)
        {
            struct Point2 points[2];
            points[0].x = origin_x + column_index * spacing_x;
            points[0].y = origin_y + 0.0;
            points[1].x = origin_x + column_index * spacing_x;
            points[1].y = origin_y + total_height;
            city_map_add_road_segment(city_map, next_segment_id++, points, 2);
        }
        else
        {
            int segment_count = 1 + (int)(random_next_u32(&random_state) % 3u);
            int segment_index = 0;
            for (segment_index = 0; segment_index < segment_count; segment_index++)
            {
                int bottom_row = random_range_int(&random_state, 0, major_rows - 2);
                int top_row = random_range_int(&random_state, bottom_row + 1, major_rows - 1);

                struct Point2 points[2];
                points[0].x = origin_x + column_index * spacing_x;
                points[0].y = origin_y + bottom_row * spacing_y;
                points[1].x = origin_x + column_index * spacing_x;
                points[1].y = origin_y + top_row * spacing_y;
                city_map_add_road_segment(city_map, next_segment_id++, points, 2);
            }
        }
    }

    city_map_add_rect_loop(
        city_map,
        next_segment_id++,
        origin_x,
        origin_y,
        origin_x + total_width,
        origin_y + total_height);

    double ring_padding = spacing_x;
    city_map_add_rect_loop(
        city_map,
        next_segment_id++,
        origin_x + ring_padding,
        origin_y + ring_padding,
        origin_x + total_width - ring_padding,
        origin_y + total_height - ring_padding);

    int center_column = major_columns / 2;
    int center_row = major_rows / 2;

    double inner_left = origin_x + center_column * spacing_x - spacing_x;
    double inner_right = origin_x + center_column * spacing_x + spacing_x;
    double inner_bottom = origin_y + center_row * spacing_y - spacing_y;
    double inner_top = origin_y + center_row * spacing_y + spacing_y;

    city_map_add_rect_loop(city_map, next_segment_id++, inner_left, inner_bottom, inner_right, inner_top);
    int neighborhood_count = 6 + (int)(random_next_u32(&random_state) % 6u);
    int neighborhood_index = 0;

    for (neighborhood_index = 0; neighborhood_index < neighborhood_count; neighborhood_index++)
    {
        int left_column = random_range_int(&random_state, 1, major_columns - 4);
        int bottom_row = random_range_int(&random_state, 1, major_rows - 4);
        int width_cells = random_range_int(&random_state, 2, 4);
        int height_cells = random_range_int(&random_state, 2, 4);

        double left = origin_x + left_column * spacing_x;
        double right = origin_x + (left_column + width_cells) * spacing_x;
        double bottom = origin_y + bottom_row * spacing_y;
        double top = origin_y + (bottom_row + height_cells) * spacing_y;

        city_map_add_rect_loop(city_map, next_segment_id++, left, bottom, right, top);
    }

    int avenue_count = (major_columns + major_rows) / 3;
    int avenue_index = 0;
    for (avenue_index = 0; avenue_index < avenue_count; avenue_index++)
    {
        int make_horizontal = (random_next_u32(&random_state) & 1u) != 0u;

        if (make_horizontal)
        {
            int row = random_range_int(&random_state, 1, major_rows - 2);
            int left_column = random_range_int(&random_state, 0, major_columns / 3);
            int right_column = random_range_int(&random_state, (2 * major_columns) / 3, major_columns - 1);

            struct Point2 points[2];
            points[0].x = origin_x + left_column * spacing_x;
            points[0].y = origin_y + row * spacing_y;
            points[1].x = origin_x + right_column * spacing_x;
            points[1].y = origin_y + row * spacing_y;

            city_map_add_road_segment(city_map, next_segment_id++, points, 2);
        }
        else
        {
            int column = random_range_int(&random_state, 1, major_columns - 2);
            int bottom_row = random_range_int(&random_state, 0, major_rows / 3);
            int top_row = random_range_int(&random_state, (2 * major_rows) / 3, major_rows - 1);

            struct Point2 points[2];
            points[0].x = origin_x + column * spacing_x;
            points[0].y = origin_y + bottom_row * spacing_y;
            points[1].x = origin_x + column * spacing_x;
            points[1].y = origin_y + top_row * spacing_y;

            city_map_add_road_segment(city_map, next_segment_id++, points, 2);
        }
    }
    int spur_budget = (major_columns * major_rows) / 6;
    int spur_index = 0;
    for (spur_index = 0; spur_index < spur_budget; spur_index++)
    {
        int base_column = random_range_int(&random_state, 0, major_columns - 1);
        int base_row = random_range_int(&random_state, 0, major_rows - 1);
        int direction = random_range_int(&random_state, 0, 3);
        int length_cells = random_range_int(&random_state, 2, 5);
        double base_x = origin_x + base_column * spacing_x;
        double base_y = origin_y + base_row * spacing_y;
        struct Point2 points[3];
        int point_count = 2;
        points[0].x = base_x;
        points[0].y = base_y;
        points[1].x = base_x;
        points[1].y = base_y;
        if (direction == 0)
            points[1].x = base_x + (double)length_cells * spacing_x * 0.5;
        if (direction == 1)
            points[1].x = base_x - (double)length_cells * spacing_x * 0.5;
        if (direction == 2)
            points[1].y = base_y + (double)length_cells * spacing_y * 0.5;
        if (direction == 3)
            points[1].y = base_y - (double)length_cells * spacing_y * 0.5;
        int make_turn = (random_next_u32(&random_state) & 1u) == 0u;
        if (make_turn)
        {
            points[2] = points[1];
            if (direction < 2)
            {
                int turn_direction = random_range_int(&random_state, 2, 3);
                if (turn_direction == 2)
                    points[2].y += spacing_y * 0.5;
                if (turn_direction == 3)
                    points[2].y -= spacing_y * 0.5;
            }
            else
            {
                int turn_direction = random_range_int(&random_state, 0, 1);
                if (turn_direction == 0)
                    points[2].x += spacing_x * 0.5;
                if (turn_direction == 1)
                    points[2].x -= spacing_x * 0.5;
            }
            point_count = 3;
        }

        double allowed_min_x = origin_x - spacing_x;
        double allowed_min_y = origin_y - spacing_y;
        double allowed_max_x = origin_x + total_width + spacing_x;
        double allowed_max_y = origin_y + total_height + spacing_y;

        if (points[1].x < allowed_min_x || points[1].x > allowed_max_x || points[1].y < allowed_min_y || points[1].y > allowed_max_y)
        {
            continue;
        }
        if (point_count == 3)
        {
            if (points[2].x < allowed_min_x || points[2].x > allowed_max_x || points[2].y < allowed_min_y || points[2].y > allowed_max_y)
            {
                continue;
            }
        }

        city_map_add_road_segment(city_map, next_segment_id++, points, point_count);
    }
}

int file_read_all(const char *path, char **out_buffer, size_t *out_length)
{
    FILE *file = fopen(path, "rb");
    if (file == NULL)
        return 0;

    if (fseek(file, 0, SEEK_END) != 0)
    {
        fclose(file);
        return 0;
    }
    long file_size = ftell(file);
    if (file_size < 0)
    {
        fclose(file);
        return 0;
    }
    if (fseek(file, 0, SEEK_SET) != 0)
    {
        fclose(file);
        return 0;
    }

    char *buffer = (char *)malloc((size_t)file_size + 1);
    if (buffer == NULL)
    {
        fclose(file);
        return 0;
    }

    size_t bytes_read = fread(buffer, 1, (size_t)file_size, file);
    fclose(file);

    buffer[bytes_read] = 0;
    *out_buffer = buffer;
    *out_length = bytes_read;
    return 1;
}

void json_initialize_parser(struct JsonParser *parser)
{
    parser->position = 0;
    parser->next_token_index = 0;
    parser->super_token_index = -1;
}

struct JsonToken *json_allocate_token(struct JsonParser *parser, struct JsonToken *tokens, size_t token_capacity)
{
    if (parser->next_token_index >= token_capacity)
        return NULL;
    struct JsonToken *token = &tokens[parser->next_token_index++];
    token->type = JSON_TOKEN_UNDEFINED;
    token->start = -1;
    token->end = -1;
    token->size = 0;
    token->parent = -1;
    return token;
}

void json_fill_token(struct JsonToken *token, enum JsonTokenType type, int start, int end)
{
    token->type = type;
    token->start = start;
    token->end = end;
    token->size = 0;
}

int json_parse_string(
    struct JsonParser *parser,
    const char *json_text,
    size_t json_length,
    struct JsonToken *tokens,
    size_t token_capacity)
{
    int start = (int)parser->position;
    parser->position++;

    for (; parser->position < json_length; parser->position++)
    {
        char c = json_text[parser->position];
        if (c == '\"')
        {
            struct JsonToken *token = json_allocate_token(parser, tokens, token_capacity);
            if (token == NULL)
                return -1;
            json_fill_token(token, JSON_TOKEN_STRING, start + 1, (int)parser->position);
            token->parent = parser->super_token_index;
            return 0;
        }
        if (c == '\\')
        {
            parser->position++;
            if (parser->position >= json_length)
                return -1;
        }
    }
    return -1;
}

int json_parse_primitive(
    struct JsonParser *parser,
    const char *json_text,
    size_t json_length,
    struct JsonToken *tokens,
    size_t token_capacity)
{
    int start = (int)parser->position;

    for (; parser->position < json_length; parser->position++)
    {
        char c = json_text[parser->position];
        if (c == '\t' || c == '\r' || c == '\n' || c == ' ' || c == ',' || c == ']' || c == '}')
            break;
        if (c < 32)
            return -1;
    }

    struct JsonToken *token = json_allocate_token(parser, tokens, token_capacity);
    if (token == NULL)
        return -1;
    json_fill_token(token, JSON_TOKEN_PRIMITIVE, start, (int)parser->position);
    token->parent = parser->super_token_index;
    parser->position--;
    return 0;
}

int json_parse(
    struct JsonParser *parser,
    const char *json_text,
    size_t json_length,
    struct JsonToken *tokens,
    unsigned int token_capacity)
{
    for (; parser->position < json_length; parser->position++)
    {
        char c = json_text[parser->position];
        struct JsonToken *token = NULL;

        if (c == '{' || c == '[')
        {
            token = json_allocate_token(parser, tokens, token_capacity);
            if (token == NULL)
                return -1;

            token->type = (c == '{') ? JSON_TOKEN_OBJECT : JSON_TOKEN_ARRAY;
            token->start = (int)parser->position;
            token->parent = parser->super_token_index;

            if (parser->super_token_index != -1)
                tokens[parser->super_token_index].size++;
            parser->super_token_index = (int)parser->next_token_index - 1;
            continue;
        }

        if (c == '}' || c == ']')
        {
            enum JsonTokenType expected_type = (c == '}') ? JSON_TOKEN_OBJECT : JSON_TOKEN_ARRAY;
            int token_index = (int)parser->next_token_index - 1;

            for (;;)
            {
                if (token_index < 0)
                    return -1;
                token = &tokens[token_index];
                if (token->start != -1 && token->end == -1)
                {
                    if (token->type != expected_type)
                        return -1;
                    token->end = (int)parser->position + 1;
                    parser->super_token_index = token->parent;
                    break;
                }
                token_index--;
            }
            continue;
        }

        if (c == '\"')
        {
            if (json_parse_string(parser, json_text, json_length, tokens, token_capacity) < 0)
                return -1;
            if (parser->super_token_index != -1)
                tokens[parser->super_token_index].size++;
            continue;
        }

        if (c == '\t' || c == '\r' || c == '\n' || c == ' ' || c == ':' || c == ',')
        {
            continue;
        }

        if (json_parse_primitive(parser, json_text, json_length, tokens, token_capacity) < 0)
            return -1;
        if (parser->super_token_index != -1)
            tokens[parser->super_token_index].size++;
    }

    unsigned int index = 0;
    for (index = 0; index < parser->next_token_index; index++)
    {
        if (tokens[index].start != -1 && tokens[index].end == -1)
            return -1;
    }
    return (int)parser->next_token_index;
}

int json_token_equals_string(const char *json_text, struct JsonToken *token, const char *expected_string)
{
    if (token->type != JSON_TOKEN_STRING)
        return 0;
    int token_length = token->end - token->start;
    int expected_length = (int)strlen(expected_string);
    if (token_length != expected_length)
        return 0;
    return strncmp(json_text + token->start, expected_string, (size_t)token_length) == 0;
}

double json_token_to_double(const char *json_text, struct JsonToken *token)
{
    int token_length = token->end - token->start;
    if (token_length <= 0)
        return 0.0;

    char buffer[64];
    if (token_length >= (int)sizeof(buffer))
        token_length = (int)sizeof(buffer) - 1;

    memcpy(buffer, json_text + token->start, (size_t)token_length);
    buffer[token_length] = 0;
    return strtod(buffer, NULL);
}

int json_token_to_int(const char *json_text, struct JsonToken *token)
{
    return (int)llround(json_token_to_double(json_text, token));
}

int city_map_load_from_json(const char *path, struct CityMap *city_map)
{
    char *json_text = NULL;
    size_t json_length = 0;
    if (!file_read_all(path, &json_text, &json_length))
        return 0;

    int token_capacity = (int)(json_length / 4 + 256);
    struct JsonToken *tokens = (struct JsonToken *)malloc((size_t)token_capacity * sizeof(struct JsonToken));
    if (tokens == NULL)
    {
        free(json_text);
        return 0;
    }

    struct JsonParser parser;
    json_initialize_parser(&parser);

    int token_count = json_parse(&parser, json_text, json_length, tokens, (unsigned int)token_capacity);
    if (token_count < 1 || tokens[0].type != JSON_TOKEN_OBJECT)
    {
        free(tokens);
        free(json_text);
        return 0;
    }

    int segments_array_index = -1;
    int index = 1;
    for (index = 1; index + 1 < token_count; index++)
    {
        if (tokens[index].parent != 0)
            continue;

        if (json_token_equals_string(json_text, &tokens[index], "segments") && tokens[index + 1].type == JSON_TOKEN_ARRAY)
        {
            segments_array_index = index + 1;
            break;
        }
    }

    if (segments_array_index < 0)
    {
        free(tokens);
        free(json_text);
        return 0;
    }

    city_map_free(city_map);
    city_map_initialize(city_map);

    struct JsonToken *segments_array_token = &tokens[segments_array_index];
    int segment_count = segments_array_token->size;

    int segment_token_index = segments_array_index + 1;
    int segment_i = 0;

    for (segment_i = 0; segment_i < segment_count; segment_i++)
    {
        if (segment_token_index >= token_count)
        {
            free(tokens);
            free(json_text);
            return 0;
        }

        struct JsonToken *segment_object_token = &tokens[segment_token_index];
        if (segment_object_token->type != JSON_TOKEN_OBJECT)
        {
            free(tokens);
            free(json_text);
            return 0;
        }

        int segment_object_index = segment_token_index;
        int segment_end = segment_object_token->end;

        int segment_id = segment_i + 1;
        int points_array_index = -1;

        int field_index = segment_object_index + 1;
        while (field_index + 1 < token_count && tokens[field_index].start < segment_end)
        {
            if (tokens[field_index].type == JSON_TOKEN_STRING && tokens[field_index].parent == segment_object_index)
            {
                struct JsonToken *key_token = &tokens[field_index];
                struct JsonToken *value_token = &tokens[field_index + 1];

                if (json_token_equals_string(json_text, key_token, "id"))
                {
                    segment_id = json_token_to_int(json_text, value_token);
                }
                else if (json_token_equals_string(json_text, key_token, "pts") && value_token->type == JSON_TOKEN_ARRAY)
                {
                    points_array_index = field_index + 1;
                }

                int value_end = value_token->end;
                int skip_index = field_index + 2;
                while (skip_index < token_count && tokens[skip_index].start < value_end)
                    skip_index++;

                field_index = skip_index;
                continue;
            }

            field_index++;
        }

        if (points_array_index < 0)
        {
            free(tokens);
            free(json_text);
            return 0;
        }

        struct JsonToken *points_array_token = &tokens[points_array_index];
        int point_count = points_array_token->size;

        struct Point2 *points = (struct Point2 *)malloc((size_t)point_count * sizeof(struct Point2));
        if (points == NULL)
        {
            free(tokens);
            free(json_text);
            return 0;
        }

        int point_token_index = points_array_index + 1;
        int point_i = 0;

        for (point_i = 0; point_i < point_count; point_i++)
        {
            if (point_token_index + 2 >= token_count)
            {
                free(points);
                free(tokens);
                free(json_text);
                return 0;
            }

            struct JsonToken *pair_token = &tokens[point_token_index];
            if (pair_token->type != JSON_TOKEN_ARRAY || pair_token->size != 2)
            {
                free(points);
                free(tokens);
                free(json_text);
                return 0;
            }

            struct JsonToken *x_token = &tokens[point_token_index + 1];
            struct JsonToken *y_token = &tokens[point_token_index + 2];

            points[point_i].x = json_token_to_double(json_text, x_token);
            points[point_i].y = json_token_to_double(json_text, y_token);

            int pair_end = pair_token->end;
            int skip_index = point_token_index + 1;
            while (skip_index < token_count && tokens[skip_index].start < pair_end)
                skip_index++;

            point_token_index = skip_index;
        }

        int ok = city_map_add_road_segment(city_map, segment_id, points, point_count);
        free(points);

        if (!ok)
        {
            free(tokens);
            free(json_text);
            return 0;
        }

        int obj_end = segment_object_token->end;
        int skip_index = segment_object_index + 1;
        while (skip_index < token_count && tokens[skip_index].start < obj_end)
            skip_index++;

        segment_token_index = skip_index;
    }

    free(tokens);
    free(json_text);
    return 1;
}

void camera_reset_to_fit_map(struct CityMap *city_map, int screen_width, int screen_height, struct Camera2DState *camera_state)
{
    double minimum_x = 0.0;
    double minimum_y = 0.0;
    double maximum_x = 1.0;
    double maximum_y = 1.0;
    city_map_compute_bounds(city_map, &minimum_x, &minimum_y, &maximum_x, &maximum_y);

    double width = maximum_x - minimum_x;
    double height = maximum_y - minimum_y;
    if (width <= 0.0)
        width = 1.0;
    if (height <= 0.0)
        height = 1.0;

    double padding_factor = 1.10;
    width *= padding_factor;
    height *= padding_factor;

    double zoom_x = (double)screen_width / width;
    double zoom_y = (double)screen_height / height;
    double zoom = zoom_x < zoom_y ? zoom_x : zoom_y;
    if (zoom <= 0.0)
        zoom = 1.0;

    double center_x = (minimum_x + maximum_x) * 0.5;
    double center_y = (minimum_y + maximum_y) * 0.5;

    camera_state->zoom = zoom;
    camera_state->offset_x = (double)screen_width * 0.5 - center_x * zoom;
    camera_state->offset_y = (double)screen_height * 0.5 - center_y * zoom;
}

Vector2 world_to_screen(struct Camera2DState *camera_state, double world_x, double world_y)
{
    float x = (float)(world_x * camera_state->zoom + camera_state->offset_x);
    float y = (float)(world_y * camera_state->zoom + camera_state->offset_y);
    Vector2 out = {x, y};
    return out;
}

void city_map_debug_render_2d_window(struct CityMap *city_map, int screen_width, int screen_height)
{
    SetConfigFlags(FLAG_MSAA_4X_HINT);
    SetTraceLogLevel(LOG_WARNING);
    InitWindow(screen_width, screen_height, "City Map Debug Render");
    SetTargetFPS(60);

    struct Camera2DState camera_state;
    camera_state.offset_x = (double)screen_width * 0.5;
    camera_state.offset_y = (double)screen_height * 0.5;
    camera_state.zoom = 1.0;

    while (!WindowShouldClose())
    {
        if (IsMouseButtonDown(MOUSE_BUTTON_MIDDLE) || IsMouseButtonDown(MOUSE_BUTTON_RIGHT))
        {
            Vector2 delta = GetMouseDelta();
            camera_state.offset_x += (double)delta.x;
            camera_state.offset_y += (double)delta.y;
        }

        float wheel = GetMouseWheelMove();
        if (wheel != 0.0f)
        {
            Vector2 mouse = GetMousePosition();
            double before_world_x = (mouse.x - camera_state.offset_x) / camera_state.zoom;
            double before_world_y = (mouse.y - camera_state.offset_y) / camera_state.zoom;

            double zoom_factor = pow(1.15, (double)wheel);
            double new_zoom = camera_state.zoom * zoom_factor;
            if (new_zoom < 1e-4)
                new_zoom = 1e-4;
            if (new_zoom > 1e6)
                new_zoom = 1e6;

            camera_state.zoom = new_zoom;

            camera_state.offset_x = mouse.x - before_world_x * camera_state.zoom;
            camera_state.offset_y = mouse.y - before_world_y * camera_state.zoom;
        }

        BeginDrawing();
        ClearBackground((Color){18, 18, 22, 255});

        int segment_index = 0;
        for (segment_index = 0; segment_index < city_map->road_segment_count; segment_index++)
        {
            struct RoadSegment *segment = &city_map->road_segments[segment_index];
            int point_index = 1;
            for (point_index = 1; point_index < segment->point_count; point_index++)
            {
                struct Point2 a = segment->points[point_index - 1];
                struct Point2 b = segment->points[point_index];

                Vector2 a_screen = world_to_screen(&camera_state, a.x, a.y);
                Vector2 b_screen = world_to_screen(&camera_state, b.x, b.y);

                float thickness = (float)(2.0f);
                DrawLineEx(a_screen, b_screen, thickness, (Color){220, 220, 230, 255});
            }
        }

        for (segment_index = 0; segment_index < city_map->road_segment_count; segment_index++)
        {
            struct RoadSegment *segment = &city_map->road_segments[segment_index];
            int point_index = 0;
            for (point_index = 0; point_index < segment->point_count; point_index++)
            {
                struct Point2 p = segment->points[point_index];
                Vector2 p_screen = world_to_screen(&camera_state, p.x, p.y);
                DrawCircleV(p_screen, 3.0f, (Color){255, 120, 120, 255});
            }
        }

        DrawText("mouse wheel: zoom | middle/right drag: pan | Esc: quit", 10, 10, 18, (Color){200, 200, 210, 255});

        EndDrawing();
    }

    CloseWindow();
}

int city_map_write_to_json(const char *path, struct CityMap *city_map)
{
    FILE *file = fopen(path, "wb");
    if (file == NULL)
        return 0;

    fprintf(file, "{\n");
    fprintf(file, "  \"segments\": [\n");

    int segment_index = 0;
    for (segment_index = 0; segment_index < city_map->road_segment_count; segment_index++)
    {
        struct RoadSegment *segment = &city_map->road_segments[segment_index];

        fprintf(file, "    {\n");
        fprintf(file, "      \"id\": %d,\n", segment->id);
        fprintf(file, "      \"pts\": [\n");

        int point_index = 0;
        for (point_index = 0; point_index < segment->point_count; point_index++)
        {
            struct Point2 *point = &segment->points[point_index];

            fprintf(
                file,
                "        [%.6f, %.6f]%s\n",
                point->x,
                point->y,
                (point_index + 1 < segment->point_count) ? "," : "");
        }

        fprintf(file, "      ]\n");
        fprintf(file, "    }%s\n",
                (segment_index + 1 < city_map->road_segment_count) ? "," : "");
    }

    fprintf(file, "  ]\n");
    fprintf(file, "}\n");

    fclose(file);
    return 1;
}

int main(int argc, char **argv)
{
    struct CityMap city_map;
    city_map_initialize(&city_map);

    if (argc >= 3 && strcmp(argv[1], "generate") == 0)
    {
        uint64_t seed = (uint64_t)strtoull(argv[2], NULL, 10);
        city_map_generate_basic_city(&city_map, seed);
        city_map_debug_render_2d_window(
            &city_map,
            city_debug_window_width_pixels(),
            city_debug_window_height_pixels());
        city_map_free(&city_map);
        return 0;
    }

    if (argc >= 4 && strcmp(argv[1], "export") == 0)
    {
        uint64_t seed = (uint64_t)strtoull(argv[2], NULL, 10);

        city_map_generate_basic_city(&city_map, seed);

        if (!city_map_write_to_json(argv[3], &city_map))
        {
            fprintf(stderr, "failed to write json\n");
            city_map_free(&city_map);
            return 1;
        }

        city_map_free(&city_map);
        return 0;
    }

    if (argc >= 3 && strcmp(argv[1], "load") == 0)
    {
        if (!city_map_load_from_json(argv[2], &city_map))
        {
            fprintf(stderr, "failed to load json\n");
            city_map_free(&city_map);
            return 1;
        }
        city_map_debug_render_2d_window(&city_map, 1200, 800);
        city_map_free(&city_map);
        return 0;
    }

    fprintf(stderr, "usage:\n");
    fprintf(stderr, "  %s generate <seed>\n", argv[0]);
    fprintf(stderr, "  %s export <seed> <out.json>\n", argv[0]);
    fprintf(stderr, "  %s load <path_to_json>\n", argv[0]);

    city_map_free(&city_map);
    return 1;
}
