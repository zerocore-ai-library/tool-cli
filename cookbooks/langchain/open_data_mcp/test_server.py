#!/usr/bin/env python3
"""
Test script for server.py
Tests every single tool and reports results.
"""

import asyncio
import json
from datetime import datetime, timedelta

from rich.console import Console
from rich.table import Table
from rich.panel import Panel

# Import all tools from the server
from server import (
    # Weather
    get_current_weather,
    get_weather_forecast,
    get_historical_weather,
    # Geo
    geocode,
    reverse_geocode,
    get_country,
    list_countries,
    get_ip_location,
    # Knowledge
    wiki_summary,
    wiki_search,
    define_word,
    get_book,
    search_books,
    # Finance
    get_crypto_price,
    list_crypto,
    convert_currency,
    get_exchange_rates,
    # News
    hn_top_stories,
    hn_story,
    reddit_posts,
    reddit_post,
    # Entertainment
    search_movies,
    get_movie,
    search_tv,
    get_tv_show,
    get_trivia,
    get_pokemon,
    search_games,
    # Science
    nasa_apod,
    get_asteroids,
    spacex_launches,
    spacex_launch,
    get_earthquakes,
    # Food
    search_recipes,
    get_recipe,
    random_recipe,
    search_cocktails,
    get_cocktail,
    get_product_nutrition,
    # Utilities
    random_user,
    random_quote,
    generate_uuid,
)

console = Console()


async def test_tool(name: str, coro, expected_keys: list[str] | None = None) -> tuple[bool, str, dict]:
    """Test a single tool and return (success, message, result)."""
    try:
        result = await coro

        # Check if result is an error
        if isinstance(result, dict) and "error" in result:
            return False, f"API error: {result['error']}", result

        # Check expected keys if provided
        if expected_keys:
            missing = [k for k in expected_keys if k not in result]
            if missing:
                return False, f"Missing keys: {missing}", result

        return True, "OK", result
    except Exception as e:
        return False, f"Exception: {str(e)}", {}


async def run_all_tests():
    """Run all tool tests."""
    console.print(Panel.fit("[bold cyan]Open Data MCP Server - Tool Tests[/]"))
    console.print()

    # Test configurations: (name, coroutine, expected_keys)
    # Using real-world test data

    yesterday = (datetime.now() - timedelta(days=30)).strftime("%Y-%m-%d")
    today = datetime.now().strftime("%Y-%m-%d")
    week_ago = (datetime.now() - timedelta(days=7)).strftime("%Y-%m-%d")

    tests = [
        # Weather (NYC coordinates)
        ("get_current_weather", get_current_weather(40.7128, -74.0060), ["temperature_c", "humidity_percent"]),
        ("get_weather_forecast", get_weather_forecast(40.7128, -74.0060, 3), ["forecast"]),
        ("get_historical_weather", get_historical_weather(40.7128, -74.0060, yesterday), ["temp_max_c"]),

        # Geo
        ("geocode", geocode("New York City"), ["latitude", "longitude"]),
        ("reverse_geocode", reverse_geocode(40.7128, -74.0060), ["display_name"]),
        ("get_country", get_country("US"), ["name", "population"]),
        ("list_countries", list_countries("Europe"), ["count", "countries"]),
        ("get_ip_location", get_ip_location(), ["ip", "country"]),

        # Knowledge
        ("wiki_summary", wiki_summary("Python_(programming_language)"), ["title", "extract"]),
        ("wiki_search", wiki_search("artificial intelligence", 5), ["results"]),
        ("define_word", define_word("serendipity"), ["word", "meanings"]),
        ("get_book", get_book("978-0-13-468599-1"), ["title"]),  # Clean Code ISBN
        ("search_books", search_books("Lord of the Rings", 5), ["results"]),

        # Finance
        ("get_crypto_price", get_crypto_price("bitcoin"), ["price_usd", "market_cap_usd"]),
        ("list_crypto", list_crypto(5), ["coins"]),
        ("convert_currency", convert_currency(100, "USD", "EUR"), ["from", "to", "rate"]),
        ("get_exchange_rates", get_exchange_rates("USD"), ["base", "rates"]),

        # News
        ("hn_top_stories", hn_top_stories(5), ["stories"]),
        ("hn_story", hn_story(1), ["id", "title"]),  # First HN story
        ("reddit_posts", reddit_posts("python", "hot", 5), ["posts"]),
        # reddit_post tested dynamically below

        # Entertainment
        ("search_movies", search_movies("Matrix"), ["results"]),
        ("get_movie", get_movie("tt0133093"), ["title", "director"]),  # The Matrix
        ("search_tv", search_tv("Breaking Bad"), ["results"]),
        ("get_tv_show", get_tv_show(169), ["name", "seasons"]),  # Breaking Bad
        ("get_trivia", get_trivia(5, 9, "easy"), ["questions"]),  # General Knowledge, easy
        ("get_pokemon", get_pokemon("pikachu"), ["name", "types", "stats"]),
        ("search_games", search_games("zelda", 5), ["query", "results"]),

        # Science
        ("nasa_apod", nasa_apod(), ["title", "explanation"]),
        ("get_asteroids", get_asteroids(week_ago, today), ["asteroids"]),
        ("spacex_launches", spacex_launches(False, 5), ["launches"]),  # Past launches
        ("spacex_launch", spacex_launch("5eb87cd9ffd86e000604b32a"), ["name"]),  # Falcon 9 launch
        ("get_earthquakes", get_earthquakes(4.5, 7, 10), ["earthquakes"]),

        # Food
        ("search_recipes", search_recipes("pasta"), ["results"]),
        ("get_recipe", get_recipe("52771"), ["name", "ingredients"]),  # Spicy Arrabiata Penne
        ("random_recipe", random_recipe(), ["name", "instructions"]),
        ("search_cocktails", search_cocktails("margarita"), ["results"]),
        ("get_cocktail", get_cocktail("11007"), ["name", "ingredients"]),  # Margarita
        ("get_product_nutrition", get_product_nutrition("737628064502"), ["name"]),  # Sample barcode

        # Utilities
        ("random_user", random_user(2), ["users"]),
        ("random_quote", random_quote(), ["content", "author"]),
        ("generate_uuid", generate_uuid(3), ["uuids"]),
    ]

    results = []
    passed = 0
    failed = 0
    reddit_post_id = None  # Will be set after reddit_posts succeeds

    for name, coro, expected in tests:
        console.print(f"  Testing [cyan]{name}[/]...", end=" ")
        success, message, result = await test_tool(name, coro, expected)

        if success:
            console.print("[green]PASS[/]")
            passed += 1
            # Capture a valid post ID from reddit_posts for dynamic testing
            if name == "reddit_posts" and result.get("posts"):
                reddit_post_id = result["posts"][0].get("id")
        else:
            console.print(f"[red]FAIL[/] - {message}")
            failed += 1

        results.append((name, success, message, result))

        # Small delay to respect rate limits
        await asyncio.sleep(0.3)

    # Dynamic test: reddit_post using a real post ID
    if reddit_post_id:
        console.print(f"  Testing [cyan]reddit_post[/] (dynamic)...", end=" ")
        success, message, result = await test_tool(
            "reddit_post",
            reddit_post("python", reddit_post_id),
            ["title"]
        )
        if success:
            console.print("[green]PASS[/]")
            passed += 1
        else:
            console.print(f"[red]FAIL[/] - {message}")
            failed += 1
        results.append(("reddit_post", success, message, result))

    # Summary
    total = passed + failed
    console.print()
    console.print(Panel.fit(f"[bold]Results: [green]{passed} passed[/], [red]{failed} failed[/] / {total} total[/]"))

    # Show failed tests detail
    if failed > 0:
        console.print("\n[bold red]Failed Tests Detail:[/]")
        for name, success, message, result in results:
            if not success:
                console.print(f"\n[red]{name}[/]:")
                console.print(f"  Message: {message}")
                if result:
                    console.print(f"  Result: {json.dumps(result, indent=2)[:500]}")

    # Show sample outputs for a few successful tests
    console.print("\n[bold cyan]Sample Outputs:[/]")
    samples = ["get_current_weather", "wiki_summary", "get_crypto_price", "get_pokemon", "random_recipe"]
    for name, success, message, result in results:
        if success and name in samples:
            console.print(f"\n[cyan]{name}[/]:")
            # Truncate long outputs
            output = json.dumps(result, indent=2)
            if len(output) > 800:
                output = output[:800] + "\n..."
            console.print(output)


if __name__ == "__main__":
    asyncio.run(run_all_tests())
